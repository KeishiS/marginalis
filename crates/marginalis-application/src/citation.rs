//! `cite:`が指す文献項目を、本文中の引用表示と参考文献項目の文字列へ直す。
//!
//! ここで作る文字列は、AsciiDocの記法ではなく表示そのものです。記法として解釈されない
//! ようにする処理は文書adapterが受け持ちます。
//!
//! CSL-JSONに無い値は補いません。著者、発行年、題名のいずれかが欠けている場合は、
//! その部分を省いた表示になります。

use marginalis_domain::BibliographyItem;
use serde_json::Value;

/// 引用と参考文献の表示規則。
///
/// ノートのheaderへ`:marginalis-citation-style:`を書いて選びます。書かないノートは
/// [`CitationStyle::default`]になります。値の正本は`NOTE_POLICY.allowed_citation_styles`に
/// あり、入力検査と`get_note_profile`の広告はどちらもそこから導きます。
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CitationStyle {
    /// 本文は`(Smith 2024)`、一覧は`Smith, A. (2024). 題名.`の形。
    #[default]
    AuthorYear,
    /// 本文は`[1]`、一覧は`[1] Smith, A. (2024). 題名.`の形。
    ///
    /// 番号は本文での初出順に振ります。同じ文献を何度引用しても同じ番号になります。
    Numeric,
}

/// 発行年が読み取れない項目の表示。
const UNKNOWN_YEAR: &str = "n.d.";

/// 著者が4名以上の場合に、先頭の姓だけを残して省略する下限。
const ET_AL_THRESHOLD: usize = 3;

impl CitationStyle {
    /// 属性へ書かれた値から表示規則を選ぶ。
    ///
    /// 値の検査は入力検査が済ませているため、ここで知らない値を受け取った場合は既定へ
    /// 落とします。表示を失敗させません。
    #[must_use]
    pub fn from_attribute(value: &str) -> Self {
        match value {
            "numeric" => Self::Numeric,
            _ => Self::default(),
        }
    }

    /// 引用全体を囲む記号。
    #[must_use]
    pub fn brackets(self) -> (&'static str, &'static str) {
        match self {
            Self::AuthorYear => ("(", ")"),
            Self::Numeric => ("[", "]"),
        }
    }

    /// 一つの引用が複数のcitation keyを名指すときの区切り。
    #[must_use]
    pub fn key_separator(self) -> &'static str {
        match self {
            Self::AuthorYear => "; ",
            Self::Numeric => ", ",
        }
    }

    /// 本文中の引用に使う短い表示を作る。
    ///
    /// `number`は本文での初出順に振った番号です。著者・年の表示では使いません。
    pub fn inline_label(self, item: &BibliographyItem, number: usize) -> String {
        if self == Self::Numeric {
            return number.to_string();
        }
        let csl = parsed(item);
        let year = year(&csl);
        match names(&csl) {
            names if names.is_empty() => match title(&csl) {
                Some(title) => format!("{title} {year}"),
                None => format!("{} {year}", item.citation_key()),
            },
            names => format!("{} {year}", short_names(&names)),
        }
    }

    /// 参考文献一覧で項目に付ける番号を決める。
    ///
    /// 番号で示すスタイルでは本文での初出順の番号を返し、文書adapterはこれを番号付き
    /// 一覧の項番として描画器へ渡します。著者・年のスタイルでは番号を付けません。
    ///
    /// 番号は`u32`で表せる範囲だけを扱います。表せない場合は`None`を返し、番号のない
    /// 一覧として描画します。1つのノートが持てる引用の数は本文の大きさで抑えられており、
    /// この範囲を超える入力は現実には作れません。
    #[must_use]
    pub fn entry_number(self, number: usize) -> Option<u32> {
        match self {
            Self::AuthorYear => None,
            Self::Numeric => u32::try_from(number).ok(),
        }
    }

    /// 参考文献一覧の1項目に使う文献情報の表示を作る。
    pub fn entry_text(self, item: &BibliographyItem) -> String {
        let csl = parsed(item);
        let mut parts = Vec::new();
        let names = names(&csl);
        if !names.is_empty() {
            parts.push(full_names(&names));
        }
        parts.push(format!("({})", year(&csl)));
        if let Some(title) = title(&csl) {
            parts.push(title);
        }
        if let Some(container) = string_field(&csl, "container-title") {
            parts.push(container);
        }
        if let Some(publisher) = string_field(&csl, "publisher") {
            parts.push(publisher);
        }
        if let Some(locator) = source_locator(&csl) {
            parts.push(locator);
        }
        if parts.len() == 1 {
            // 年しか読み取れない場合でも、どの項目かは分かるようにする。
            parts.insert(0, item.citation_key().to_owned());
        }
        parts
            .iter()
            .map(|part| ended_with_period(part))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// 項目の区切りとして文末の句点を1つだけ置く。
///
/// 名の頭文字のように既に句点で終わる部分へ重ねると`A..`になるため、有無を見て決める。
fn ended_with_period(part: &str) -> String {
    if part.ends_with('.') {
        part.to_owned()
    } else {
        format!("{part}.")
    }
}

/// 保存済みのCSL-JSONを読み取る。
///
/// 登録時に構文を検証しているため、ここで読み取れない場合は空の値として扱い、
/// citation keyだけで表示します。描画を失敗させません。
fn parsed(item: &BibliographyItem) -> Value {
    serde_json::from_str(item.csl_json()).unwrap_or(Value::Null)
}

/// 著者、無ければ編者の名前を表示順に返す。
fn names(csl: &Value) -> Vec<PersonName> {
    ["author", "editor"]
        .into_iter()
        .find_map(|field| {
            let entries = csl.get(field)?.as_array()?;
            let names = entries.iter().filter_map(person_name).collect::<Vec<_>>();
            (!names.is_empty()).then_some(names)
        })
        .unwrap_or_default()
}

/// CSL-JSONの1名分の名前。`literal`だけを持つ団体名も同じ型で扱う。
struct PersonName {
    family: String,
    given: Option<String>,
}

fn person_name(value: &Value) -> Option<PersonName> {
    if let Some(literal) = value.get("literal").and_then(Value::as_str) {
        return non_empty(literal).map(|family| PersonName {
            family,
            given: None,
        });
    }
    let family = non_empty(value.get("family").and_then(Value::as_str)?)?;
    Some(PersonName {
        family,
        given: value
            .get("given")
            .and_then(Value::as_str)
            .and_then(non_empty),
    })
}

/// 本文中に出す姓の並び。3名までは全員、4名以上は先頭だけを残す。
fn short_names(names: &[PersonName]) -> String {
    match names {
        [only] => only.family.clone(),
        [first, second] => format!("{} & {}", first.family, second.family),
        [first, ..] if names.len() > ET_AL_THRESHOLD => format!("{} et al.", first.family),
        [initial @ .., last] => format!(
            "{} & {}",
            initial
                .iter()
                .map(|name| name.family.as_str())
                .collect::<Vec<_>>()
                .join(", "),
            last.family
        ),
        [] => String::new(),
    }
}

/// 一覧に出す名前の並び。名は頭文字だけにする。
fn full_names(names: &[PersonName]) -> String {
    let formatted = names
        .iter()
        .map(|name| match &name.given {
            Some(given) => format!("{}, {}", name.family, initials(given)),
            None => name.family.clone(),
        })
        .collect::<Vec<_>>();
    match formatted.as_slice() {
        [only] => only.clone(),
        [initial @ .., last] => format!("{}, & {last}", initial.join(", ")),
        [] => String::new(),
    }
}

/// 名を頭文字と句点の並びへ直す。`Alex Mary`は`A. M.`になる。
fn initials(given: &str) -> String {
    given
        .split([' ', '-', '.'])
        .filter_map(|part| part.chars().next())
        .map(|initial| format!("{initial}."))
        .collect::<Vec<_>>()
        .join(" ")
}

/// 発行年。読み取れない場合は`n.d.`を返す。
fn year(csl: &Value) -> String {
    let Some(issued) = csl.get("issued") else {
        return UNKNOWN_YEAR.into();
    };
    let from_parts = issued
        .get("date-parts")
        .and_then(Value::as_array)
        .and_then(|parts| parts.first())
        .and_then(Value::as_array)
        .and_then(|part| part.first())
        .and_then(|year| match year {
            Value::Number(number) => Some(number.to_string()),
            Value::String(text) => non_empty(text),
            _ => None,
        });
    from_parts
        .or_else(|| {
            issued
                .get("literal")
                .and_then(Value::as_str)
                .and_then(non_empty)
        })
        .unwrap_or_else(|| UNKNOWN_YEAR.into())
}

fn title(csl: &Value) -> Option<String> {
    string_field(csl, "title")
}

/// 文献そのものの所在。DOIを優先し、無ければURLを使う。
fn source_locator(csl: &Value) -> Option<String> {
    if let Some(doi) = string_field(csl, "DOI") {
        return Some(format!("https://doi.org/{doi}"));
    }
    string_field(csl, "URL")
}

fn string_field(csl: &Value, name: &str) -> Option<String> {
    csl.get(name).and_then(Value::as_str).and_then(non_empty)
}

fn non_empty(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

#[cfg(test)]
mod tests {
    use marginalis_domain::{
        BibliographyItemId, EntityId, Identity, PrincipalId, PrincipalRef, UnixMillis,
    };
    use std::str::FromStr;

    use super::*;

    fn item(mut csl: Value) -> BibliographyItem {
        let object = csl.as_object_mut().expect("CSL-JSON object");
        object
            .entry("id")
            .or_insert_with(|| Value::String("key".into()));
        object
            .entry("type")
            .or_insert_with(|| Value::String("book".into()));
        BibliographyItem::create(
            BibliographyItemId::new(
                EntityId::from_str("0197c9bc-0000-7000-8000-000000000010").expect("UUIDv7"),
            ),
            &PrincipalRef::new(
                PrincipalId::new(1).expect("ID"),
                Identity::new("https://id.example.test".into(), "alice".into()).expect("owner"),
            ),
            marginalis_domain::ValidatedCslJson::new(&csl).expect("valid CSL-JSON"),
            UnixMillis::new(0),
        )
    }

    #[test]
    fn one_author_uses_the_family_name_and_the_year() {
        let item = item(serde_json::json!({
            "id": "smith2024",
            "type": "article-journal",
            "title": "An Example Article",
            "author": [{ "family": "Smith", "given": "Alex" }],
            "issued": { "date-parts": [[2024, 5, 1]] }
        }));

        assert_eq!(
            CitationStyle::AuthorYear.inline_label(&item, 1),
            "Smith 2024"
        );
        assert_eq!(
            CitationStyle::AuthorYear.entry_text(&item),
            "Smith, A. (2024). An Example Article."
        );
    }

    #[test]
    fn two_authors_are_joined_and_more_are_shortened() {
        let two = item(serde_json::json!({
            "id": "pair",
            "author": [{ "family": "Smith" }, { "family": "Tanaka" }],
            "issued": { "date-parts": [[2024]] }
        }));
        let many = item(serde_json::json!({
            "id": "many",
            "author": [
                { "family": "Smith" },
                { "family": "Tanaka" },
                { "family": "Ito" },
                { "family": "Mori" }
            ],
            "issued": { "date-parts": [[2024]] }
        }));

        assert_eq!(
            CitationStyle::AuthorYear.inline_label(&two, 1),
            "Smith & Tanaka 2024"
        );
        assert_eq!(
            CitationStyle::AuthorYear.inline_label(&many, 1),
            "Smith et al. 2024"
        );
        assert_eq!(
            CitationStyle::AuthorYear.entry_text(&two),
            "Smith, & Tanaka. (2024)."
        );
    }

    #[test]
    fn missing_values_are_omitted_and_never_invented() {
        let bare = item(serde_json::json!({ "id": "bare", "type": "book" }));

        assert_eq!(
            CitationStyle::AuthorYear.inline_label(&bare, 1),
            "bare n.d."
        );
        assert_eq!(CitationStyle::AuthorYear.entry_text(&bare), "bare. (n.d.).");
    }

    #[test]
    fn an_organisation_keeps_its_literal_name() {
        let organisation = item(serde_json::json!({
            "id": "org2023",
            "title": "Annual Report",
            "author": [{ "literal": "Example Institute" }],
            "issued": { "literal": "2023" },
            "URL": "https://example.test/report"
        }));

        assert_eq!(
            CitationStyle::AuthorYear.inline_label(&organisation, 1),
            "Example Institute 2023"
        );
        assert_eq!(
            CitationStyle::AuthorYear.entry_text(&organisation),
            "Example Institute. (2023). Annual Report. https://example.test/report."
        );
    }

    #[test]
    fn a_doi_is_shown_as_a_resolvable_address() {
        let with_doi = item(serde_json::json!({
            "id": "doi2022",
            "title": "Example",
            "author": [{ "family": "Smith", "given": "Alex Mary" }],
            "issued": { "date-parts": [["2022"]] },
            "container-title": "Example Journal",
            "DOI": "10.1234/example"
        }));

        assert_eq!(
            CitationStyle::AuthorYear.entry_text(&with_doi),
            "Smith, A. M. (2022). Example. Example Journal. https://doi.org/10.1234/example."
        );
    }

    #[test]
    fn a_title_stands_in_when_no_name_is_recorded() {
        let anonymous = item(serde_json::json!({
            "id": "anon",
            "title": "Untitled Work",
            "issued": { "date-parts": [[2020]] }
        }));

        assert_eq!(
            CitationStyle::AuthorYear.inline_label(&anonymous, 1),
            "Untitled Work 2020"
        );
    }
}
