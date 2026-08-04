//! ノート所有者ごとのMathJaxマクロ設定。

use std::sync::Arc;

use async_trait::async_trait;
use marginalis_domain::{Actor, Identity};

pub const MAX_MATH_MACROS: usize = 64;
pub const MAX_MATH_MACRO_NAME_CHARACTERS: usize = 32;
pub const MAX_MATH_MACRO_REPLACEMENT_CHARACTERS: usize = 512;
pub const MAX_MATH_MACRO_ARGUMENTS: u8 = 9;
pub const MAX_MATH_MACRO_TOTAL_BYTES: usize = 16 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MathMacro {
    pub name: String,
    pub replacement: String,
    pub argument_count: u8,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MathMacroSettings {
    pub macros: Vec<MathMacro>,
    /// 未保存の既定設定は0、保存済みの設定は1以上です。
    pub revision: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum MathMacroRepositoryError {
    #[error("math macro settings conflict")]
    Conflict,
    #[error("stored math macro settings are invalid")]
    CorruptData,
    #[error("math macro settings storage is unavailable")]
    Unavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum MathMacroUseCaseError {
    #[error("math macro settings are invalid")]
    Invalid,
    #[error("math macro settings conflict")]
    Conflict,
    #[error("math macro settings are unavailable")]
    Unavailable,
    #[error("stored math macro settings are invalid")]
    CorruptData,
}

#[async_trait]
pub trait MathMacroRepository: Send + Sync {
    async fn read_math_macros(
        &self,
        owner: &Identity,
    ) -> Result<MathMacroSettings, MathMacroRepositoryError>;

    async fn replace_math_macros(
        &self,
        owner: &Identity,
        macros: &[MathMacro],
        expected_revision: i64,
    ) -> Result<MathMacroSettings, MathMacroRepositoryError>;
}

#[async_trait]
pub trait MathMacroUseCases: Send + Sync {
    async fn read_math_macros(
        &self,
        actor: Actor,
    ) -> Result<MathMacroSettings, MathMacroUseCaseError>;

    async fn replace_math_macros(
        &self,
        actor: Actor,
        macros: Vec<MathMacro>,
        expected_revision: i64,
    ) -> Result<MathMacroSettings, MathMacroUseCaseError>;
}

pub struct MathMacroApplication {
    repository: Arc<dyn MathMacroRepository>,
}

impl MathMacroApplication {
    pub fn new(repository: Arc<dyn MathMacroRepository>) -> Self {
        Self { repository }
    }
}

#[async_trait]
impl MathMacroUseCases for MathMacroApplication {
    async fn read_math_macros(
        &self,
        actor: Actor,
    ) -> Result<MathMacroSettings, MathMacroUseCaseError> {
        self.repository
            .read_math_macros(actor.identity())
            .await
            .map_err(map_repository_error)
    }

    async fn replace_math_macros(
        &self,
        actor: Actor,
        macros: Vec<MathMacro>,
        expected_revision: i64,
    ) -> Result<MathMacroSettings, MathMacroUseCaseError> {
        validate_math_macros(&macros)?;
        if expected_revision < 0 {
            return Err(MathMacroUseCaseError::Invalid);
        }
        self.repository
            .replace_math_macros(actor.identity(), &macros, expected_revision)
            .await
            .map_err(map_repository_error)
    }
}

pub fn validate_math_macros(macros: &[MathMacro]) -> Result<(), MathMacroUseCaseError> {
    if macros.len() > MAX_MATH_MACROS
        || macros
            .iter()
            .map(|item| item.name.len() + item.replacement.len())
            .sum::<usize>()
            > MAX_MATH_MACRO_TOTAL_BYTES
    {
        return Err(MathMacroUseCaseError::Invalid);
    }
    let mut names = std::collections::HashSet::new();
    for item in macros {
        if item.name.is_empty()
            || item.name.chars().count() > MAX_MATH_MACRO_NAME_CHARACTERS
            || !item.name.bytes().all(|byte| byte.is_ascii_alphabetic())
            || item.replacement.is_empty()
            || item.replacement.chars().count() > MAX_MATH_MACRO_REPLACEMENT_CHARACTERS
            || item.replacement.chars().any(char::is_control)
            || item.argument_count > MAX_MATH_MACRO_ARGUMENTS
            || !names.insert(item.name.as_str())
            || !valid_argument_references(&item.replacement, item.argument_count)
        {
            return Err(MathMacroUseCaseError::Invalid);
        }
    }
    Ok(())
}

fn valid_argument_references(replacement: &str, argument_count: u8) -> bool {
    let bytes = replacement.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'#' {
            index += 1;
            continue;
        }
        let Some(number) = bytes.get(index + 1).and_then(|byte| byte.checked_sub(b'0')) else {
            return false;
        };
        if number == 0 || number > argument_count {
            return false;
        }
        index += 2;
    }
    true
}

fn map_repository_error(error: MathMacroRepositoryError) -> MathMacroUseCaseError {
    match error {
        MathMacroRepositoryError::Conflict => MathMacroUseCaseError::Conflict,
        MathMacroRepositoryError::CorruptData => MathMacroUseCaseError::CorruptData,
        MathMacroRepositoryError::Unavailable => MathMacroUseCaseError::Unavailable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_argmax_and_one_argument_bold_alias() {
        assert_eq!(
            validate_math_macros(&[
                MathMacro {
                    name: "argmax".into(),
                    replacement: r"\operatorname*{arg\,max}".into(),
                    argument_count: 0,
                },
                MathMacro {
                    name: "bm".into(),
                    replacement: r"\boldsymbol{#1}".into(),
                    argument_count: 1,
                },
            ]),
            Ok(())
        );
    }

    #[test]
    fn rejects_invalid_names_duplicates_and_argument_references() {
        for macros in [
            vec![MathMacro {
                name: r"\bm".into(),
                replacement: r"\boldsymbol{#1}".into(),
                argument_count: 1,
            }],
            vec![
                MathMacro {
                    name: "bm".into(),
                    replacement: "x".into(),
                    argument_count: 0,
                },
                MathMacro {
                    name: "bm".into(),
                    replacement: "y".into(),
                    argument_count: 0,
                },
            ],
            vec![MathMacro {
                name: "bm".into(),
                replacement: r"\boldsymbol{#2}".into(),
                argument_count: 1,
            }],
        ] {
            assert_eq!(
                validate_math_macros(&macros),
                Err(MathMacroUseCaseError::Invalid)
            );
        }
    }
}
