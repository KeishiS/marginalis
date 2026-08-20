//! 所有者によるノートACLの読み取りと置き換え。

use std::collections::HashSet;

use marginalis_domain::{
    Actor, Identity, Note, NoteAclEntry, NoteId, NoteValidationTarget, Revision,
};

use crate::{
    NoteAclChange, NoteAclState, NoteUseCaseError, NoteValidationCode, NoteValidationDiagnostic,
};

use super::NoteApplication;

impl NoteApplication {
    pub async fn read_note_acl(
        &self,
        actor: Actor,
        note_id: NoteId,
    ) -> Result<NoteAclState, NoteUseCaseError> {
        self.access_control
            .read_note_acl(&actor, note_id)
            .await
            .map_err(NoteUseCaseError::from)
    }

    pub async fn replace_note_acl(
        &self,
        actor: Actor,
        note_id: NoteId,
        entries: Vec<NoteAclChange>,
        expected_revision: Revision,
    ) -> Result<Note, NoteUseCaseError> {
        let note = self.read_visible_note(&actor, note_id).await?;
        let mut grants = Vec::with_capacity(entries.len());
        let mut identities = HashSet::with_capacity(entries.len());
        let mut principals = HashSet::with_capacity(entries.len());
        for (index, entry) in entries.iter().enumerate() {
            let identity = Identity::new(entry.issuer.clone(), entry.subject.clone())
                .map_err(|_| acl_validation(index, AclValidationProblem::InvalidSubject))?;
            if identity.issuer() != self.acl_issuer {
                return Err(acl_validation(index, AclValidationProblem::InvalidIssuer));
            }
            if !identities.insert(identity.clone()) {
                return Err(acl_validation(
                    index,
                    AclValidationProblem::DuplicateSubject,
                ));
            }
            let principal = self
                .principals
                .resolve_or_create_acl_target(identity)
                .await
                .map_err(NoteUseCaseError::from)?;
            if principal.id() == note.owner().id() {
                return Err(acl_validation(index, AclValidationProblem::OwnerIncluded));
            }
            // 同じprincipalへ結び付いた複数identityを別々のACL項目として受け取ると、
            // DBの一意制約に依存した不明瞭な失敗になる。外部identityが異なっていても
            // 実際の認可対象が同じなら入力段階で重複として拒否する。
            if !principals.insert(principal.id()) {
                return Err(acl_validation(
                    index,
                    AclValidationProblem::DuplicateSubject,
                ));
            }
            grants.push(NoteAclEntry::new(principal, entry.permission));
        }
        self.access_control
            .replace_note_acl(
                &actor,
                note_id,
                &grants,
                expected_revision,
                self.clock.now(),
            )
            .await
            .map_err(NoteUseCaseError::from)
    }
}

/// ACL入力だけで起こる問題。
///
/// 全体の`NoteValidationCode`を受け取ると、ACLでは起こらない値も型の上では渡せてしまう。
/// 到達しないことをコメントで主張せず、渡せる値を型で限定する。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AclValidationProblem {
    InvalidIssuer,
    InvalidSubject,
    DuplicateSubject,
    OwnerIncluded,
}

impl AclValidationProblem {
    const fn code(self) -> NoteValidationCode {
        match self {
            Self::InvalidIssuer => NoteValidationCode::InvalidAclIssuer,
            Self::InvalidSubject => NoteValidationCode::InvalidAclSubject,
            Self::DuplicateSubject => NoteValidationCode::DuplicateAclSubject,
            Self::OwnerIncluded => NoteValidationCode::OwnerInAcl,
        }
    }

    const fn message(self) -> &'static str {
        match self {
            Self::InvalidIssuer => "ACL issuer is not the configured OIDC issuer",
            Self::InvalidSubject => "ACL subject is invalid",
            Self::DuplicateSubject => "ACL subject is duplicated",
            Self::OwnerIncluded => "note owner must not be included in ACL",
        }
    }
}

fn acl_validation(index: usize, problem: AclValidationProblem) -> NoteUseCaseError {
    NoteUseCaseError::Validation(vec![NoteValidationDiagnostic {
        code: problem.code().as_str().into(),
        target: NoteValidationTarget::AclEntry { index },
        span: None,
        position: None,
        message: problem.message().into(),
    }])
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use marginalis_domain::{NoteCreationSource, NoteDraft, NotePermission};

    use crate::{NoteValidationTarget, NoteWritePolicy};

    use super::*;
    use crate::notes::test_support::{
        AcceptContent, EmptyLibrary, MemoryNotes, NoMathMacros, actor, note_application,
    };

    async fn owned_note() -> (NoteApplication, Actor, Note) {
        let repository = Arc::new(MemoryNotes::default());
        let application = note_application(
            &repository,
            Arc::new(AcceptContent::default()),
            Arc::new(EmptyLibrary),
            Arc::new(NoMathMacros),
        );
        let owner = actor("alice", 1);
        let note = application
            .create_note(
                owner.clone(),
                NoteDraft {
                    source: "= ACL試験\n\n本文".into(),
                    title: "ACL試験".into(),
                    tags: Vec::new(),
                },
                NoteWritePolicy::AllowAdvisories,
                NoteCreationSource::Rest,
            )
            .await
            .expect("create note");
        (application, owner, note)
    }

    #[tokio::test]
    async fn validation_reports_the_original_acl_entry_index() {
        let (application, owner, note) = owned_note().await;
        let error = application
            .replace_note_acl(
                owner,
                note.note_id(),
                vec![
                    NoteAclChange {
                        issuer: "https://id.example.test".into(),
                        subject: "zed".into(),
                        permission: NotePermission::Read,
                    },
                    NoteAclChange {
                        issuer: "https://0-invalid.example.test".into(),
                        subject: "alice".into(),
                        permission: NotePermission::Read,
                    },
                ],
                note.revision(),
            )
            .await
            .expect_err("invalid issuer");
        let NoteUseCaseError::Validation(diagnostics) = error else {
            panic!("ACL validation error is expected: {error:?}");
        };
        assert_eq!(diagnostics[0].code, "invalid_acl_issuer");
        assert_eq!(
            diagnostics[0].target,
            NoteValidationTarget::AclEntry { index: 1 }
        );
    }

    #[tokio::test]
    async fn aliases_of_one_principal_are_rejected_as_duplicate_acl_entries() {
        let (application, owner, note) = owned_note().await;
        let error = application
            .replace_note_acl(
                owner,
                note.note_id(),
                vec![
                    NoteAclChange {
                        issuer: "https://id.example.test".into(),
                        subject: "first-alias".into(),
                        permission: NotePermission::Read,
                    },
                    NoteAclChange {
                        issuer: "https://id.example.test".into(),
                        subject: "second-alias".into(),
                        permission: NotePermission::Edit,
                    },
                ],
                note.revision(),
            )
            .await
            .expect_err("one principal must have one ACL entry");
        let NoteUseCaseError::Validation(diagnostics) = error else {
            panic!("ACL validation error is expected: {error:?}");
        };
        assert_eq!(diagnostics[0].code, "duplicate_acl_subject");
        assert_eq!(
            diagnostics[0].target,
            NoteValidationTarget::AclEntry { index: 1 }
        );
    }
}
