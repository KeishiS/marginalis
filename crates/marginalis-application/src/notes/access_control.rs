//! 所有者によるノートACLの読み取りと置き換え。

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
        mut entries: Vec<NoteAclChange>,
        expected_revision: Revision,
    ) -> Result<Note, NoteUseCaseError> {
        let note = self.read_visible_note(&actor, note_id).await?;
        entries.sort_by(|left, right| left.subject.cmp(&right.subject));
        let mut grants = Vec::with_capacity(entries.len());
        for (index, entry) in entries.iter().enumerate() {
            let identity =
                Identity::new(note.creator_issuer().to_owned(), entry.subject.clone())
                    .map_err(|_| acl_validation(index, AclValidationProblem::InvalidSubject))?;
            if entry.subject == note.creator_subject() {
                return Err(acl_validation(index, AclValidationProblem::OwnerIncluded));
            }
            if index > 0 && entries[index - 1].subject == entry.subject {
                return Err(acl_validation(
                    index,
                    AclValidationProblem::DuplicateSubject,
                ));
            }
            grants.push(NoteAclEntry::new(identity, entry.permission));
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
    InvalidSubject,
    DuplicateSubject,
    OwnerIncluded,
}

impl AclValidationProblem {
    const fn code(self) -> NoteValidationCode {
        match self {
            Self::InvalidSubject => NoteValidationCode::InvalidAclSubject,
            Self::DuplicateSubject => NoteValidationCode::DuplicateAclSubject,
            Self::OwnerIncluded => NoteValidationCode::OwnerInAcl,
        }
    }

    const fn message(self) -> &'static str {
        match self {
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
