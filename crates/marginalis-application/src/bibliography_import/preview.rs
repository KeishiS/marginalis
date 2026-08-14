//! 取込入力と保存状態の比較、および事前確認tokenの生成。

use std::collections::HashSet;

use marginalis_domain::{BibliographyImportMethod, BibliographyItem};
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::{
    BibliographyImportCandidate, BibliographyImportClassification, BibliographyImportEntry,
    BibliographyImportInput, BibliographyImportPreview, BibliographyImportSourceSelection,
    BibliographyImportState, BibliographyImportUseCaseError, validate_item,
};

pub(super) fn classify_import(
    input: &BibliographyImportInput,
    state: &BibliographyImportState,
) -> Result<BibliographyImportPreview, BibliographyImportUseCaseError> {
    let mut external_ids = HashSet::new();
    let entries = input
        .items
        .iter()
        .enumerate()
        .map(|(position, value)| classify_entry(position, value, state, &mut external_ids))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(BibliographyImportPreview {
        source_id: state.source.as_ref().map(|source| source.source_id()),
        source_revision: state.source.as_ref().map(|source| source.revision()),
        preview_token: import_state_token(input, state)?,
        entries,
    })
}

/// 入力と、分類に影響する読み取り状態を結び付ける事前確認token。
///
/// 署名や認可tokenではない。適用時に同じ値を再計算し、事前確認後に入力、取込元、対応、
/// 所有者の文献ライブラリのいずれかが変わった場合に、全体を変更せず再確認を求める。
pub(super) fn import_state_token(
    input: &BibliographyImportInput,
    state: &BibliographyImportState,
) -> Result<String, BibliographyImportUseCaseError> {
    let mut hasher = Sha256::new();
    hash_component(&mut hasher, b"marginalis-bibliography-import-preview-1");
    match &input.source {
        BibliographyImportSourceSelection::New { display_name } => {
            hash_component(&mut hasher, b"new");
            hash_component(&mut hasher, display_name.as_bytes());
        }
        BibliographyImportSourceSelection::Existing { source_id } => {
            hash_component(&mut hasher, b"existing");
            hash_component(&mut hasher, source_id.to_string().as_bytes());
        }
    }
    let encoded_items = serde_json::to_vec(&input.items)
        .map_err(|_| BibliographyImportUseCaseError::InvalidInput("invalid_json"))?;
    hash_component(&mut hasher, &encoded_items);

    if let Some(source) = &state.source {
        hash_component(&mut hasher, source.source_id().to_string().as_bytes());
        hash_component(&mut hasher, source.owner().issuer().as_bytes());
        hash_component(&mut hasher, source.owner().subject().as_bytes());
        hash_component(
            &mut hasher,
            match source.method() {
                BibliographyImportMethod::CslJsonFile => b"csl_json_file",
            },
        );
        hash_component(&mut hasher, source.display_name().as_bytes());
        hash_component(&mut hasher, &source.revision().get().to_be_bytes());
        hash_component(&mut hasher, &source.created_at().get().to_be_bytes());
        hash_component(&mut hasher, &source.last_imported_at().get().to_be_bytes());
    } else {
        hash_component(&mut hasher, b"no-source");
    }

    let mut links = state.links.iter().collect::<Vec<_>>();
    links.sort_by(|left, right| {
        (left.source_id().to_string(), left.external_item_id())
            .cmp(&(right.source_id().to_string(), right.external_item_id()))
    });
    for link in links {
        hash_component(&mut hasher, link.source_id().to_string().as_bytes());
        hash_component(&mut hasher, link.external_item_id().as_bytes());
        hash_component(&mut hasher, link.item_id().to_string().as_bytes());
        hash_component(&mut hasher, link.imported_digest().as_bytes());
        hash_component(
            &mut hasher,
            &link.imported_item_revision().get().to_be_bytes(),
        );
    }

    let mut items = state.items.iter().collect::<Vec<_>>();
    items.sort_by_key(|item| item.item_id().to_string());
    for item in items {
        hash_component(&mut hasher, item.item_id().to_string().as_bytes());
        hash_component(&mut hasher, &item.revision().get().to_be_bytes());
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn hash_component(hasher: &mut Sha256, value: &[u8]) {
    hasher.update(u64::try_from(value.len()).unwrap_or(u64::MAX).to_be_bytes());
    hasher.update(value);
}

pub(super) fn is_canonical_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn classify_entry(
    position: usize,
    value: &Value,
    state: &BibliographyImportState,
    external_ids: &mut HashSet<String>,
) -> Result<BibliographyImportEntry, BibliographyImportUseCaseError> {
    let validated = match validate_item(value) {
        Ok(validated) => validated,
        Err(code) => return Ok(rejected_entry(position, code)),
    };
    if !external_ids.insert(validated.external_item_id.clone()) {
        return Ok(rejected_entry(position, "duplicate_external_item_id"));
    }
    if let Some(link) = state
        .links
        .iter()
        .find(|link| link.external_item_id() == validated.external_item_id)
    {
        let Some(item) = state
            .items
            .iter()
            .find(|item| item.item_id() == link.item_id())
        else {
            return Ok(rejected_entry(position, "stored_link_target_missing"));
        };
        let current_csl_json = serde_json::from_str(item.csl_json())
            .map_err(|_| BibliographyImportUseCaseError::CorruptData)?;
        let external_changed = validated.digest != link.imported_digest();
        let local_changed = item.revision() != link.imported_item_revision();
        let classification = match (external_changed, local_changed) {
            (false, false) => BibliographyImportClassification::Unchanged,
            (true, false) => BibliographyImportClassification::UpdateFromExternal,
            (false, true) => BibliographyImportClassification::KeepLocal,
            (true, true) => BibliographyImportClassification::Conflict,
        };
        return Ok(BibliographyImportEntry {
            position,
            external_item_id: Some(validated.external_item_id),
            citation_key: Some(item.citation_key().to_owned()),
            classification,
            item_id: Some(item.item_id()),
            item_revision: Some(item.revision()),
            current_csl_json: Some(current_csl_json),
            candidates: Vec::new(),
            rejection_code: None,
        });
    }

    let candidates = duplicate_candidates(value, validated.csl_json.citation_key(), &state.items);
    Ok(BibliographyImportEntry {
        position,
        external_item_id: Some(validated.external_item_id),
        citation_key: Some(validated.csl_json.citation_key().to_owned()),
        classification: if candidates.is_empty() {
            BibliographyImportClassification::Create
        } else {
            BibliographyImportClassification::DuplicateCandidate
        },
        item_id: None,
        item_revision: None,
        current_csl_json: None,
        candidates,
        rejection_code: None,
    })
}

fn rejected_entry(position: usize, code: &'static str) -> BibliographyImportEntry {
    BibliographyImportEntry {
        position,
        external_item_id: None,
        citation_key: None,
        classification: BibliographyImportClassification::Rejected,
        item_id: None,
        item_revision: None,
        current_csl_json: None,
        candidates: Vec::new(),
        rejection_code: Some(code.into()),
    }
}

fn duplicate_candidates(
    input: &Value,
    citation_key: &str,
    items: &[BibliographyItem],
) -> Vec<BibliographyImportCandidate> {
    let mut candidates = items
        .iter()
        .filter_map(|item| {
            let stored = serde_json::from_str::<Value>(item.csl_json()).ok()?;
            let matched_by = matching_fields(input, &stored, citation_key, item.citation_key());
            (!matched_by.is_empty()).then(|| BibliographyImportCandidate {
                item_id: item.item_id(),
                citation_key: item.citation_key().to_owned(),
                title: stored
                    .get("title")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                revision: item.revision(),
                matched_by,
            })
        })
        .collect::<Vec<_>>();
    candidates.sort_unstable_by_key(|candidate| candidate.item_id.to_string());
    candidates
}

fn matching_fields(
    input: &Value,
    stored: &Value,
    input_key: &str,
    stored_key: &str,
) -> Vec<String> {
    let mut matched = Vec::new();
    if input_key == stored_key {
        matched.push("citation_key".into());
    }
    for key in ["DOI", "ISBN", "PMID", "PMCID", "URL"] {
        let left = input
            .get(key)
            .and_then(Value::as_str)
            .and_then(normalized_match);
        let right = stored
            .get(key)
            .and_then(Value::as_str)
            .and_then(normalized_match);
        if left.is_some() && left == right {
            matched.push(key.to_ascii_lowercase());
        }
    }
    let left_title = input
        .get("title")
        .and_then(Value::as_str)
        .and_then(normalized_match);
    let right_title = stored
        .get("title")
        .and_then(Value::as_str)
        .and_then(normalized_match);
    if left_title.is_some() && left_title == right_title {
        matched.push("title".into());
    }
    matched
}

fn normalized_match(value: &str) -> Option<String> {
    let value = value.trim().to_lowercase();
    (!value.is_empty()).then_some(value)
}
