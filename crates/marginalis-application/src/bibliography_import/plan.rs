//! 利用者が選んだ事前確認結果から、原子的な保存計画を組み立てる。

use std::collections::HashSet;

use marginalis_domain::{
    BibliographyContentDigest, BibliographyImportLink, BibliographyImportSource, BibliographyItem,
    BibliographyItemId, Revision, UnixMillis,
};
use serde_json::Value;

use crate::Random;

use super::{
    BibliographyImportClassification, BibliographyImportCommit, BibliographyImportDecision,
    BibliographyImportDecisionKind, BibliographyImportEntry, BibliographyImportInput,
    BibliographyImportItemMutation, BibliographyImportPreview, BibliographyImportState,
    BibliographyImportUseCaseError, ValidatedItem, validate_item,
};

#[allow(clippy::too_many_arguments)]
pub(super) fn build_commit(
    input: &BibliographyImportInput,
    preview: &BibliographyImportPreview,
    expected_state: BibliographyImportState,
    source: BibliographyImportSource,
    decisions: Vec<BibliographyImportDecision>,
    imported_at: UnixMillis,
    random: &dyn Random,
) -> Result<BibliographyImportCommit, BibliographyImportUseCaseError> {
    let items = &expected_state.items;
    let mut seen_positions = HashSet::new();
    if decisions
        .iter()
        .any(|decision| !seen_positions.insert(decision.position))
    {
        return Err(BibliographyImportUseCaseError::InvalidDecision);
    }
    let mut mutations = Vec::new();
    let mut target_item_ids = HashSet::new();
    let mut excluded = 0;
    for entry in &preview.entries {
        let decision = decisions
            .iter()
            .find(|decision| decision.position == entry.position)
            .ok_or(BibliographyImportUseCaseError::InvalidDecision)?;
        let candidate_required = matches!(
            decision.kind,
            BibliographyImportDecisionKind::LinkExistingKeepLocal
                | BibliographyImportDecisionKind::LinkExistingUseExternal
        );
        if candidate_required != decision.candidate_item_id.is_some() {
            return Err(BibliographyImportUseCaseError::InvalidDecision);
        }
        if decision.kind == BibliographyImportDecisionKind::Exclude {
            excluded += 1;
            continue;
        }
        if entry.classification == BibliographyImportClassification::Rejected {
            return Err(BibliographyImportUseCaseError::InvalidDecision);
        }
        let validated = validate_item(&input.items[entry.position])
            .map_err(BibliographyImportUseCaseError::InvalidInput)?;
        let mutation = mutation_for_decision(
            entry,
            decision,
            validated,
            items,
            &source,
            imported_at,
            random,
        )?;
        let target_item_id = match &mutation {
            BibliographyImportItemMutation::Create { item, .. } => item.item_id(),
            BibliographyImportItemMutation::Update { item_id, .. } => *item_id,
            BibliographyImportItemMutation::Keep { link, .. } => link.item_id(),
        };
        if !target_item_ids.insert(target_item_id) {
            return Err(BibliographyImportUseCaseError::InvalidDecision);
        }
        mutations.push(mutation);
    }
    if decisions.len() != preview.entries.len() || mutations.is_empty() {
        return Err(BibliographyImportUseCaseError::InvalidDecision);
    }
    Ok(BibliographyImportCommit {
        source,
        expected_state,
        imported_at,
        mutations,
        excluded,
    })
}

fn mutation_for_decision(
    entry: &BibliographyImportEntry,
    decision: &BibliographyImportDecision,
    validated: ValidatedItem,
    items: &[BibliographyItem],
    source: &BibliographyImportSource,
    imported_at: UnixMillis,
    random: &dyn Random,
) -> Result<BibliographyImportItemMutation, BibliographyImportUseCaseError> {
    match (entry.classification, decision.kind) {
        (
            BibliographyImportClassification::Create,
            BibliographyImportDecisionKind::ApplySuggested,
        ) => create_mutation(validated, source, imported_at, random),
        (
            BibliographyImportClassification::DuplicateCandidate,
            BibliographyImportDecisionKind::CreateSeparate,
        ) if can_create_separate(entry) => create_mutation(validated, source, imported_at, random),
        (
            BibliographyImportClassification::UpdateFromExternal,
            BibliographyImportDecisionKind::ApplySuggested,
        )
        | (
            BibliographyImportClassification::Conflict,
            BibliographyImportDecisionKind::UseExternal,
        ) => update_mutation(entry, validated, source, imported_at),
        (
            BibliographyImportClassification::Unchanged,
            BibliographyImportDecisionKind::ApplySuggested,
        )
        | (
            BibliographyImportClassification::KeepLocal,
            BibliographyImportDecisionKind::ApplySuggested,
        )
        | (BibliographyImportClassification::Conflict, BibliographyImportDecisionKind::KeepLocal) => {
            keep_mutation(entry, validated, source)
        }
        (
            BibliographyImportClassification::DuplicateCandidate,
            BibliographyImportDecisionKind::LinkExistingKeepLocal,
        )
        | (
            BibliographyImportClassification::DuplicateCandidate,
            BibliographyImportDecisionKind::LinkExistingUseExternal,
        ) => link_candidate_mutation(entry, decision, validated, items, source, imported_at),
        _ => Err(BibliographyImportUseCaseError::InvalidDecision),
    }
}

fn create_mutation(
    validated: ValidatedItem,
    source: &BibliographyImportSource,
    imported_at: UnixMillis,
    random: &dyn Random,
) -> Result<BibliographyImportItemMutation, BibliographyImportUseCaseError> {
    let item = BibliographyItem::create(
        BibliographyItemId::new(random.uuid_v7()),
        source.owner(),
        validated.csl_json,
        imported_at,
    );
    let link = link(
        source,
        &validated.external_item_id,
        item.item_id(),
        validated.digest,
        item.revision(),
    )?;
    Ok(BibliographyImportItemMutation::Create { item, link })
}

fn link_candidate_mutation(
    entry: &BibliographyImportEntry,
    decision: &BibliographyImportDecision,
    validated: ValidatedItem,
    items: &[BibliographyItem],
    source: &BibliographyImportSource,
    imported_at: UnixMillis,
) -> Result<BibliographyImportItemMutation, BibliographyImportUseCaseError> {
    let candidate_id = decision
        .candidate_item_id
        .ok_or(BibliographyImportUseCaseError::InvalidDecision)?;
    if !entry
        .candidates
        .iter()
        .any(|candidate| candidate.item_id == candidate_id)
    {
        return Err(BibliographyImportUseCaseError::InvalidDecision);
    }
    let candidate = items
        .iter()
        .find(|item| item.item_id() == candidate_id)
        .ok_or(BibliographyImportUseCaseError::Conflict)?;
    let linked_entry = BibliographyImportEntry {
        item_id: Some(candidate.item_id()),
        item_revision: Some(candidate.revision()),
        current_csl_json: serde_json::from_str(candidate.csl_json()).ok(),
        citation_key: Some(candidate.citation_key().to_owned()),
        ..entry.clone()
    };
    if decision.kind == BibliographyImportDecisionKind::LinkExistingUseExternal {
        update_mutation(&linked_entry, validated, source, imported_at)
    } else {
        keep_mutation(&linked_entry, validated, source)
    }
}

pub(super) fn can_create_separate(entry: &BibliographyImportEntry) -> bool {
    !entry.candidates.iter().any(|candidate| {
        candidate
            .matched_by
            .iter()
            .any(|field| field == "citation_key")
    })
}

fn update_mutation(
    entry: &BibliographyImportEntry,
    mut validated: ValidatedItem,
    source: &BibliographyImportSource,
    imported_at: UnixMillis,
) -> Result<BibliographyImportItemMutation, BibliographyImportUseCaseError> {
    let item_id = entry
        .item_id
        .ok_or(BibliographyImportUseCaseError::Conflict)?;
    let revision = entry
        .item_revision
        .ok_or(BibliographyImportUseCaseError::Conflict)?;
    let citation_key = entry
        .citation_key
        .as_ref()
        .ok_or(BibliographyImportUseCaseError::Conflict)?;
    let object = serde_json::from_str::<Value>(validated.csl_json.encoded())
        .map_err(|_| BibliographyImportUseCaseError::InvalidInput("invalid_json"))?;
    let mut object =
        object
            .as_object()
            .cloned()
            .ok_or(BibliographyImportUseCaseError::InvalidInput(
                "item_not_object",
            ))?;
    object.insert("id".into(), Value::String(citation_key.clone()));
    validated.csl_json = marginalis_domain::ValidatedCslJson::new(&Value::Object(object))
        .map_err(|_| BibliographyImportUseCaseError::InvalidInput("invalid_csl_json"))?;
    let next_revision = revision
        .get()
        .checked_add(1)
        .and_then(|value| Revision::new(value).ok())
        .ok_or(BibliographyImportUseCaseError::CorruptData)?;
    let link = link(
        source,
        &validated.external_item_id,
        item_id,
        validated.digest,
        next_revision,
    )?;
    Ok(BibliographyImportItemMutation::Update {
        item_id,
        csl_json: validated.csl_json,
        expected_revision: revision,
        link,
        updated_at: imported_at,
    })
}

fn keep_mutation(
    entry: &BibliographyImportEntry,
    validated: ValidatedItem,
    source: &BibliographyImportSource,
) -> Result<BibliographyImportItemMutation, BibliographyImportUseCaseError> {
    let item_id = entry
        .item_id
        .ok_or(BibliographyImportUseCaseError::Conflict)?;
    let revision = entry
        .item_revision
        .ok_or(BibliographyImportUseCaseError::Conflict)?;
    Ok(BibliographyImportItemMutation::Keep {
        expected_revision: revision,
        link: link(
            source,
            &validated.external_item_id,
            item_id,
            validated.digest,
            revision,
        )?,
    })
}

fn link(
    source: &BibliographyImportSource,
    external_item_id: &str,
    item_id: BibliographyItemId,
    digest: BibliographyContentDigest,
    revision: Revision,
) -> Result<BibliographyImportLink, BibliographyImportUseCaseError> {
    BibliographyImportLink::new(
        source.source_id(),
        external_item_id.to_owned(),
        item_id,
        digest,
        revision,
    )
    .map_err(|_| BibliographyImportUseCaseError::InvalidInput("invalid_external_item_id"))
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use marginalis_domain::{BibliographyImportSourceId, EntityId, Identity};

    use super::*;
    use crate::bibliography_import::BibliographyImportSourceSelection;

    struct FixedRandom;

    impl Random for FixedRandom {
        fn uuid_v7(&self) -> EntityId {
            EntityId::from_str("0197c9bc-0000-7000-8000-0000000000d3").expect("UUIDv7")
        }

        fn opaque_token(&self) -> String {
            unreachable!("bibliography import does not issue opaque tokens")
        }
    }

    fn owner() -> Identity {
        Identity::new("https://id.example.test".into(), "alice".into()).expect("owner")
    }

    fn source() -> BibliographyImportSource {
        BibliographyImportSource::create(
            BibliographyImportSourceId::new(
                EntityId::from_str("0197c9bc-0000-7000-8000-0000000000d1").expect("UUIDv7"),
            ),
            &owner(),
            "Zotero".into(),
            UnixMillis::new(10),
        )
        .expect("source")
    }

    fn item(id: &str, citation_key: &str) -> BibliographyItem {
        BibliographyItem::restore(
            BibliographyItemId::new(EntityId::from_str(id).expect("UUIDv7")),
            owner(),
            citation_key.into(),
            format!(r#"{{"id":"{citation_key}","title":"Local","type":"book"}}"#),
            UnixMillis::new(10),
            UnixMillis::new(20),
            Revision::new(2).expect("revision"),
        )
        .expect("item")
    }

    #[test]
    fn external_update_preserves_the_local_key_and_never_mutates_absent_items() {
        let linked = item("0197c9bc-0000-7000-8000-0000000000d2", "local-key");
        let absent = item("0197c9bc-0000-7000-8000-0000000000d4", "absent-key");
        let input = BibliographyImportInput {
            source: BibliographyImportSourceSelection::Existing {
                source_id: source().source_id(),
            },
            items: vec![serde_json::json!({
                "id": "external-key", "title": "External", "type": "book"
            })],
        };
        let preview = BibliographyImportPreview {
            source_id: Some(source().source_id()),
            source_revision: Some(Revision::INITIAL),
            preview_token: "a".repeat(64),
            entries: vec![BibliographyImportEntry {
                position: 0,
                external_item_id: Some("external-key".into()),
                citation_key: Some("local-key".into()),
                classification: BibliographyImportClassification::Conflict,
                item_id: Some(linked.item_id()),
                item_revision: Some(linked.revision()),
                current_csl_json: None,
                candidates: Vec::new(),
                rejection_code: None,
            }],
        };
        let commit = build_commit(
            &input,
            &preview,
            BibliographyImportState {
                source: Some(source()),
                links: Vec::new(),
                items: vec![linked, absent],
            },
            source(),
            vec![BibliographyImportDecision {
                position: 0,
                kind: BibliographyImportDecisionKind::UseExternal,
                candidate_item_id: None,
            }],
            UnixMillis::new(30),
            &FixedRandom,
        )
        .expect("commit");

        assert_eq!(commit.mutations.len(), 1);
        let BibliographyImportItemMutation::Update { csl_json, link, .. } = &commit.mutations[0]
        else {
            panic!("expected update")
        };
        let csl_json: Value = serde_json::from_str(csl_json.encoded()).expect("CSL-JSON");
        assert_eq!(csl_json["id"], "local-key");
        assert_eq!(link.external_item_id(), "external-key");
    }

    #[test]
    fn excluding_every_entry_is_not_a_persistable_plan() {
        let input = BibliographyImportInput {
            source: BibliographyImportSourceSelection::New {
                display_name: "Zotero".into(),
            },
            items: vec![serde_json::json!({"id": "one", "type": "book"})],
        };
        let preview = BibliographyImportPreview {
            source_id: None,
            source_revision: None,
            preview_token: "a".repeat(64),
            entries: vec![BibliographyImportEntry {
                position: 0,
                external_item_id: Some("one".into()),
                citation_key: Some("one".into()),
                classification: BibliographyImportClassification::Create,
                item_id: None,
                item_revision: None,
                current_csl_json: None,
                candidates: Vec::new(),
                rejection_code: None,
            }],
        };
        assert_eq!(
            build_commit(
                &input,
                &preview,
                BibliographyImportState {
                    source: None,
                    links: Vec::new(),
                    items: Vec::new(),
                },
                source(),
                vec![BibliographyImportDecision {
                    position: 0,
                    kind: BibliographyImportDecisionKind::Exclude,
                    candidate_item_id: None,
                }],
                UnixMillis::new(30),
                &FixedRandom,
            ),
            Err(BibliographyImportUseCaseError::InvalidDecision)
        );

        assert_eq!(
            build_commit(
                &input,
                &preview,
                BibliographyImportState {
                    source: None,
                    links: Vec::new(),
                    items: Vec::new(),
                },
                source(),
                vec![BibliographyImportDecision {
                    position: 0,
                    kind: BibliographyImportDecisionKind::ApplySuggested,
                    candidate_item_id: Some(BibliographyItemId::new(
                        EntityId::from_str("0197c9bc-0000-7000-8000-0000000000d5").expect("UUIDv7"),
                    )),
                }],
                UnixMillis::new(30),
                &FixedRandom,
            ),
            Err(BibliographyImportUseCaseError::InvalidDecision)
        );
    }

    #[test]
    fn one_plan_cannot_target_the_same_bibliography_item_twice() {
        let existing = item("0197c9bc-0000-7000-8000-0000000000d2", "local-key");
        let state = BibliographyImportState {
            source: Some(source()),
            links: Vec::new(),
            items: vec![existing.clone()],
        };
        let input = BibliographyImportInput {
            source: BibliographyImportSourceSelection::Existing {
                source_id: source().source_id(),
            },
            items: vec![
                serde_json::json!({"id": "external-one", "title": "Local", "type": "book"}),
                serde_json::json!({"id": "external-two", "title": "Local", "type": "book"}),
            ],
        };
        let preview = crate::bibliography_import::classify_import(&input, &state).expect("preview");
        let decisions = [0, 1]
            .into_iter()
            .map(|position| BibliographyImportDecision {
                position,
                kind: BibliographyImportDecisionKind::LinkExistingKeepLocal,
                candidate_item_id: Some(existing.item_id()),
            })
            .collect();

        assert_eq!(
            build_commit(
                &input,
                &preview,
                state,
                source(),
                decisions,
                UnixMillis::new(30),
                &FixedRandom,
            ),
            Err(BibliographyImportUseCaseError::InvalidDecision)
        );
    }
}
