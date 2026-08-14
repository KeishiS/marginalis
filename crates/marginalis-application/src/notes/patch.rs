//! Unified Diffの厳密な適用。
//!
//! `expected_revision`の検査と適用後の全文検証はユースケース側が行い、このmoduleは
//! 「patchの解釈」と「保存済み原文への適用」だけを担う。適用は厳密で、hunkに記録された
//! 変更前の行と文脈行が指定位置へ完全一致した場合だけ変更する。位置をずらして一致箇所を
//! 探すことはしない。
//!
//! 受理するのは単一ファイルのUnified Diffだけで、file headerは`--- a/note.adoc`と
//! `+++ b/note.adoc`の固定名を要求する。行は`\n`区切りの1始まりで数え、`\r`を含む行は
//! byte単位でそのまま比較する。

use marginalis_domain::NOTE_POLICY;

/// patchを適用できない理由。位置は利用者が自分のpatchを直せるように返す。
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum NotePatchError {
    /// patch全体がUTF-8バイト数の上限を超えている。
    #[error("patch exceeds the size limit")]
    PatchTooLarge,
    /// hunk数が上限を超えている。
    #[error("patch contains too many hunks")]
    TooManyHunks,
    /// Unified Diffとして解釈できない。`line`はpatch内の1始まりの行番号。
    #[error("patch line {line} is not valid unified diff syntax")]
    InvalidFormat { line: usize },
    /// file headerが`a/note.adoc`・`b/note.adoc`の単一ファイルでない。
    #[error("patch must target exactly a/note.adoc and b/note.adoc")]
    UnsupportedHeader,
    /// hunkの位置と行数が前のhunkと重なるか、順序が逆転している。
    #[error("patch hunk {hunk} overlaps a previous hunk or is out of order")]
    HunkOutOfOrder { hunk: usize },
    /// hunkが指す行が保存済み原文に存在しない。
    #[error("patch hunk {hunk} points beyond the end of the stored source")]
    HunkOutOfRange { hunk: usize },
    /// hunkの変更前の行または文脈行が、保存済み原文の指定位置と一致しない。
    /// `source_line`は保存済み原文側の1始まりの行番号。
    #[error("patch hunk {hunk} does not match the stored source at line {source_line}")]
    HunkMismatch { hunk: usize, source_line: usize },
}

/// 適用の結果。変更後の原文と、応答へ載せる変更量。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NotePatchOutcome {
    pub source: String,
    pub hunks_applied: usize,
    pub lines_added: usize,
    pub lines_removed: usize,
}

/// hunk本文の1行。文脈、削除、追加のいずれか。
#[derive(Clone, Debug, Eq, PartialEq)]
enum HunkLine {
    Context(String),
    Remove(String),
    Add(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Hunk {
    old_start: usize,
    old_len: usize,
    new_len: usize,
    lines: Vec<HunkLine>,
    /// 変更前側の末尾行が「改行なしで終わる」印を持つか。
    old_ends_without_newline: bool,
    /// 変更後側の末尾行が「改行なしで終わる」印を持つか。
    new_ends_without_newline: bool,
}

/// Unified Diffを保存済み原文へ厳密に適用する。
pub fn apply_note_patch(source: &str, patch: &str) -> Result<NotePatchOutcome, NotePatchError> {
    if patch.len() > NOTE_POLICY.max_patch_bytes {
        return Err(NotePatchError::PatchTooLarge);
    }
    let hunks = parse_patch(patch)?;
    if hunks.len() > NOTE_POLICY.max_patch_hunks {
        return Err(NotePatchError::TooManyHunks);
    }

    // 原文を末尾改行の有無つきの行列へ分解する。
    let had_trailing_newline = source.ends_with('\n');
    let source_lines: Vec<&str> = if source.is_empty() {
        Vec::new()
    } else if had_trailing_newline {
        let mut lines: Vec<&str> = source.split('\n').collect();
        lines.pop();
        lines
    } else {
        source.split('\n').collect()
    };

    let mut output: Vec<String> = Vec::new();
    let mut output_ends_without_newline = source_lines.is_empty() || !had_trailing_newline;
    // 次に写す原文の位置(0始まり)。
    let mut cursor = 0usize;
    let mut lines_added = 0usize;
    let mut lines_removed = 0usize;

    for (index, hunk) in hunks.iter().enumerate() {
        let hunk_number = index + 1;
        // old_len == 0の挿入hunkでは、old_startは「その行の後ろへ挿入する」を意味する。
        let first_line = if hunk.old_len == 0 {
            hunk.old_start
        } else {
            hunk.old_start
                .checked_sub(1)
                .ok_or(NotePatchError::HunkOutOfRange { hunk: hunk_number })?
        };
        if first_line < cursor {
            return Err(NotePatchError::HunkOutOfOrder { hunk: hunk_number });
        }
        if first_line
            .checked_add(hunk.old_len)
            .is_none_or(|end| end > source_lines.len())
        {
            return Err(NotePatchError::HunkOutOfRange { hunk: hunk_number });
        }

        // hunkの手前までを無変更で写す。
        output.extend(
            source_lines[cursor..first_line]
                .iter()
                .map(|&line| line.to_owned()),
        );
        cursor = first_line;

        // 変更前側の印は、原文の末尾改行の状態と一致しなければならない。
        let covers_source_end = cursor + hunk.old_len == source_lines.len() && hunk.old_len > 0;
        if hunk.old_ends_without_newline && !(covers_source_end && !had_trailing_newline) {
            return Err(NotePatchError::HunkMismatch {
                hunk: hunk_number,
                source_line: source_lines.len(),
            });
        }
        if covers_source_end && !had_trailing_newline && !hunk.old_ends_without_newline {
            return Err(NotePatchError::HunkMismatch {
                hunk: hunk_number,
                source_line: source_lines.len(),
            });
        }

        for line in &hunk.lines {
            match line {
                HunkLine::Context(expected) => {
                    if source_lines.get(cursor) != Some(&expected.as_str()) {
                        return Err(NotePatchError::HunkMismatch {
                            hunk: hunk_number,
                            source_line: cursor + 1,
                        });
                    }
                    output.push(expected.clone());
                    cursor += 1;
                }
                HunkLine::Remove(expected) => {
                    if source_lines.get(cursor) != Some(&expected.as_str()) {
                        return Err(NotePatchError::HunkMismatch {
                            hunk: hunk_number,
                            source_line: cursor + 1,
                        });
                    }
                    lines_removed += 1;
                    cursor += 1;
                }
                HunkLine::Add(added) => {
                    lines_added += 1;
                    output.push(added.clone());
                }
            }
        }

        // 変更後側の末尾改行は、原文の末尾を書き換えたhunkだけが決められる。
        if cursor == source_lines.len() {
            output_ends_without_newline = hunk.new_ends_without_newline;
        }
    }

    output.extend(source_lines[cursor..].iter().map(|&line| line.to_owned()));

    let mut new_source = output.join("\n");
    if !new_source.is_empty() && !output_ends_without_newline {
        new_source.push('\n');
    }
    Ok(NotePatchOutcome {
        source: new_source,
        hunks_applied: hunks.len(),
        lines_added,
        lines_removed,
    })
}

/// Unified Diffを解釈する。単一ファイル・固定file header・順序どおりのhunkだけを受理する。
fn parse_patch(patch: &str) -> Result<Vec<Hunk>, NotePatchError> {
    if patch.is_empty() {
        return Err(NotePatchError::InvalidFormat { line: 1 });
    }
    // `str::lines()`は行末の`\r`を落とすため使わない。`\r`を含む行を
    // byte単位で保つよう、`\n`だけで区切り、末尾の1つの空行を除く。
    let body = patch.strip_suffix('\n').unwrap_or(patch);
    let mut lines = body.split('\n').enumerate().peekable();

    let (_, old_header) = lines
        .next()
        .ok_or(NotePatchError::InvalidFormat { line: 1 })?;
    if old_header.trim_end() != "--- a/note.adoc" {
        return Err(NotePatchError::UnsupportedHeader);
    }
    let (_, new_header) = lines
        .next()
        .ok_or(NotePatchError::InvalidFormat { line: 2 })?;
    if new_header.trim_end() != "+++ b/note.adoc" {
        return Err(NotePatchError::UnsupportedHeader);
    }

    let mut hunks: Vec<Hunk> = Vec::new();
    while let Some((index, header)) = lines.next() {
        let line_number = index + 1;
        // 2つ目以降のfile headerは複数ファイルのpatchであり、受理しない。
        if header.starts_with("--- ") || header.starts_with("+++ ") || header.starts_with("diff ") {
            return Err(NotePatchError::UnsupportedHeader);
        }
        let (old_start, old_len, new_len) =
            parse_hunk_header(header).ok_or(NotePatchError::InvalidFormat { line: line_number })?;

        let mut hunk = Hunk {
            old_start,
            old_len,
            new_len,
            lines: Vec::new(),
            old_ends_without_newline: false,
            new_ends_without_newline: false,
        };
        let mut seen_old = 0usize;
        let mut seen_new = 0usize;
        while seen_old < old_len || seen_new < new_len {
            let (index, body) = lines
                .next()
                .ok_or(NotePatchError::InvalidFormat { line: line_number })?;
            let body_number = index + 1;
            match body.chars().next() {
                Some(' ') => {
                    seen_old += 1;
                    seen_new += 1;
                    hunk.lines.push(HunkLine::Context(body[1..].to_owned()));
                }
                Some('-') => {
                    seen_old += 1;
                    hunk.lines.push(HunkLine::Remove(body[1..].to_owned()));
                }
                Some('+') => {
                    seen_new += 1;
                    hunk.lines.push(HunkLine::Add(body[1..].to_owned()));
                }
                // 空文字列の文脈行。`git diff`は末尾空白を保つため通常現れないが、
                // 行末の空白を落とす転送系で作られたpatchを機械的に拒否しない。
                None => {
                    seen_old += 1;
                    seen_new += 1;
                    hunk.lines.push(HunkLine::Context(String::new()));
                }
                _ => return Err(NotePatchError::InvalidFormat { line: body_number }),
            }
            // 「改行なしで終わる」印は、直前の行の側に付く。
            if let Some((_, marker)) = lines.peek()
                && marker.starts_with('\\')
            {
                let last = hunk
                    .lines
                    .last()
                    .expect("a marker always follows a hunk line");
                match last {
                    HunkLine::Context(_) => {
                        hunk.old_ends_without_newline = true;
                        hunk.new_ends_without_newline = true;
                    }
                    HunkLine::Remove(_) => hunk.old_ends_without_newline = true,
                    HunkLine::Add(_) => hunk.new_ends_without_newline = true,
                }
                lines.next();
            }
        }
        hunks.push(hunk);
    }

    if hunks.is_empty() {
        return Err(NotePatchError::InvalidFormat { line: 3 });
    }
    Ok(hunks)
}

/// `@@ -l[,s] +l[,s] @@`のhunk headerを読む。末尾の節名は無視する。
fn parse_hunk_header(header: &str) -> Option<(usize, usize, usize)> {
    let rest = header.strip_prefix("@@ -")?;
    let (old_part, rest) = rest.split_once(" +")?;
    let (new_part, _) = rest.split_once(" @@")?;
    let (old_start, old_len) = parse_range(old_part)?;
    let (_, new_len) = parse_range(new_part)?;
    Some((old_start, old_len, new_len))
}

/// `l[,s]`を(開始行, 行数)として読む。行数を省略した場合は1。
fn parse_range(range: &str) -> Option<(usize, usize)> {
    match range.split_once(',') {
        Some((start, len)) => Some((start.parse().ok()?, len.parse().ok()?)),
        None => Some((range.parse().ok()?, 1)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SOURCE: &str = "= Title\n\n== Section\n\nfirst\nsecond\nthird\n";

    fn patch(body: &str) -> String {
        format!("--- a/note.adoc\n+++ b/note.adoc\n{body}")
    }

    /// 文脈が一致する1 hunkを適用し、変更量を数える。
    #[test]
    fn applies_a_matching_hunk_exactly() {
        let outcome = apply_note_patch(
            SOURCE,
            &patch("@@ -5,3 +5,3 @@\n first\n-second\n+SECOND\n third\n"),
        )
        .expect("apply");
        assert_eq!(
            outcome.source,
            "= Title\n\n== Section\n\nfirst\nSECOND\nthird\n"
        );
        assert_eq!(
            (
                outcome.hunks_applied,
                outcome.lines_added,
                outcome.lines_removed
            ),
            (1, 1, 1)
        );
    }

    /// 複数hunkは順序どおりに適用し、間の行は変更しない。
    #[test]
    fn applies_multiple_hunks_in_order() {
        let outcome = apply_note_patch(
            SOURCE,
            &patch(concat!(
                "@@ -1,1 +1,1 @@\n-= Title\n+= New Title\n",
                "@@ -7,1 +7,2 @@\n third\n+fourth\n",
            )),
        )
        .expect("apply");
        assert_eq!(
            outcome.source,
            "= New Title\n\n== Section\n\nfirst\nsecond\nthird\nfourth\n"
        );
        assert_eq!(outcome.hunks_applied, 2);
    }

    /// 行数を省略したhunk header(`-l +l`)は1行として読む。
    #[test]
    fn accepts_headers_without_an_explicit_length() {
        let outcome =
            apply_note_patch(SOURCE, &patch("@@ -5 +5 @@\n-first\n+FIRST\n")).expect("apply");
        assert!(outcome.source.contains("FIRST\nsecond"));
    }

    /// 空文書への挿入は`-0,0`を受理する。
    #[test]
    fn inserts_into_an_empty_source() {
        let outcome =
            apply_note_patch("", &patch("@@ -0,0 +1,2 @@\n+= Title\n+\n")).expect("apply");
        assert_eq!(outcome.source, "= Title\n\n");
    }

    /// 文脈や変更前の行が一致しない場合は、位置をずらして再探索せず拒否する。
    #[test]
    fn rejects_a_hunk_whose_context_does_not_match() {
        let error = apply_note_patch(
            SOURCE,
            &patch("@@ -5,3 +5,3 @@\n first\n-SECOND\n+second\n third\n"),
        )
        .expect_err("mismatch");
        assert_eq!(
            error,
            NotePatchError::HunkMismatch {
                hunk: 1,
                source_line: 6
            }
        );
    }

    /// 一致する内容でも、行番号が保存済み原文とずれている場合は拒否する(fuzz禁止)。
    #[test]
    fn rejects_a_hunk_at_a_shifted_line_number() {
        let error = apply_note_patch(
            SOURCE,
            &patch("@@ -4,3 +4,3 @@\n first\n-second\n+SECOND\n third\n"),
        )
        .expect_err("shifted");
        assert_eq!(
            error,
            NotePatchError::HunkMismatch {
                hunk: 1,
                source_line: 4
            }
        );
    }

    /// 原文の範囲外を指すhunkは拒否する。
    #[test]
    fn rejects_a_hunk_beyond_the_source() {
        let error = apply_note_patch(SOURCE, &patch("@@ -9,1 +9,1 @@\n-x\n+y\n"))
            .expect_err("out of range");
        assert_eq!(error, NotePatchError::HunkOutOfRange { hunk: 1 });
    }

    /// 逆順・重複するhunkは拒否する。
    #[test]
    fn rejects_overlapping_or_unordered_hunks() {
        let error = apply_note_patch(
            SOURCE,
            &patch(concat!(
                "@@ -5,2 +5,2 @@\n first\n-second\n+SECOND\n",
                "@@ -5,1 +5,1 @@\n-first\n+FIRST\n",
            )),
        )
        .expect_err("overlap");
        assert_eq!(error, NotePatchError::HunkOutOfOrder { hunk: 2 });
    }

    /// 固定file header以外(別名・複数ファイル)は拒否する。
    #[test]
    fn rejects_headers_for_other_files() {
        let error = apply_note_patch(
            SOURCE,
            "--- a/other.adoc\n+++ b/other.adoc\n@@ -1,1 +1,1 @@\n-= Title\n+= T\n",
        )
        .expect_err("other file");
        assert_eq!(error, NotePatchError::UnsupportedHeader);

        let error = apply_note_patch(
            SOURCE,
            &patch(concat!(
                "@@ -1,1 +1,1 @@\n-= Title\n+= T\n",
                "--- a/note.adoc\n+++ b/note.adoc\n@@ -1,1 +1,1 @@\n-x\n+y\n",
            )),
        )
        .expect_err("second file");
        assert_eq!(error, NotePatchError::UnsupportedHeader);
    }

    /// hunkの行数宣言と本文が食い違う場合は形式不正として拒否する。
    #[test]
    fn rejects_a_truncated_hunk() {
        let error = apply_note_patch(SOURCE, &patch("@@ -5,3 +5,3 @@\n first\n-second\n"))
            .expect_err("truncated");
        assert!(matches!(error, NotePatchError::InvalidFormat { .. }));
    }

    /// `\r\n`の行はbyte単位で比較し、`\r`を含む文脈が一致すれば適用する。
    #[test]
    fn compares_crlf_lines_byte_for_byte() {
        let source = "alpha\r\nbeta\r\n";
        let outcome = apply_note_patch(
            source,
            &patch("@@ -1,2 +1,2 @@\n alpha\r\n-beta\r\n+BETA\r\n"),
        )
        .expect("apply");
        assert_eq!(outcome.source, "alpha\r\nBETA\r\n");

        // `\r`を落としたpatchは一致しない。
        let error = apply_note_patch(source, &patch("@@ -1,2 +1,2 @@\n alpha\n-beta\n+BETA\n"))
            .expect_err("missing carriage return");
        assert!(matches!(error, NotePatchError::HunkMismatch { .. }));
    }

    /// 末尾改行のない原文は、`\ No newline at end of file`の印が必要になる。
    #[test]
    fn tracks_missing_trailing_newlines_explicitly() {
        let source = "alpha\nomega";
        let outcome = apply_note_patch(
            source,
            &patch("@@ -2,1 +2,1 @@\n-omega\n\\ No newline at end of file\n+OMEGA\n"),
        )
        .expect("apply");
        assert_eq!(outcome.source, "alpha\nOMEGA\n");

        // 印なしで末尾行を変更するpatchは、原文の末尾改行の状態と一致しない。
        let error = apply_note_patch(source, &patch("@@ -2,1 +2,1 @@\n-omega\n+OMEGA\n"))
            .expect_err("missing marker");
        assert!(matches!(error, NotePatchError::HunkMismatch { .. }));

        // 変更後側の印は、出力の末尾改行を消す。
        let outcome = apply_note_patch(
            "alpha\nomega\n",
            &patch("@@ -2,1 +2,1 @@\n-omega\n+OMEGA\n\\ No newline at end of file\n"),
        )
        .expect("apply without trailing newline");
        assert_eq!(outcome.source, "alpha\nOMEGA");
    }

    /// 上限(バイト数・hunk数)を超えるpatchは適用前に拒否する。
    #[test]
    fn enforces_size_and_hunk_limits() {
        let oversized = patch(&format!(
            "@@ -5,1 +5,1 @@\n-first\n+{}\n",
            "x".repeat(NOTE_POLICY.max_patch_bytes)
        ));
        assert_eq!(
            apply_note_patch(SOURCE, &oversized).expect_err("too large"),
            NotePatchError::PatchTooLarge
        );

        let mut body = String::new();
        let mut source = String::new();
        for index in 0..NOTE_POLICY.max_patch_hunks + 1 {
            source.push_str(&format!("line{index}\nkeep{index}\n"));
            body.push_str(&format!(
                "@@ -{0},1 +{0},1 @@\n-line{1}\n+LINE{1}\n",
                index * 2 + 1,
                index
            ));
        }
        assert_eq!(
            apply_note_patch(&source, &patch(&body)).expect_err("too many hunks"),
            NotePatchError::TooManyHunks
        );
    }

    /// file headerとhunkのないpatchは形式不正として拒否する。
    #[test]
    fn rejects_empty_patches() {
        assert!(matches!(
            apply_note_patch(SOURCE, "").expect_err("empty"),
            NotePatchError::InvalidFormat { line: 1 }
        ));
        assert!(matches!(
            apply_note_patch(SOURCE, "--- a/note.adoc\n+++ b/note.adoc\n").expect_err("no hunks"),
            NotePatchError::InvalidFormat { .. }
        ));
    }
}
