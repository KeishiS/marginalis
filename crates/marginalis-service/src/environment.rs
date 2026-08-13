//! 環境変数の宣言と読み取り。
//!
//! 起動時の設定構築（[`crate::config`]）と`diagnose`（[`crate::maintenance`]）は、どちらも
//! このmoduleの宣言と読み取り関数を使用する。両者が変数名や「未設定」の判定を別々に持つと、
//! 診断が報告する状態と実際の起動判断が食い違うため、判定を一か所に閉じる。

use std::env;

pub(crate) const BASE_URL: &str = "MARGINALIS_BASE_URL";
pub(crate) const DATABASE_URL: &str = "MARGINALIS_DATABASE_URL";
pub(crate) const LISTEN_ADDRESS: &str = "MARGINALIS_LISTEN_ADDR";
pub(crate) const OIDC_ISSUER_URL: &str = "MARGINALIS_OIDC_ISSUER_URL";
pub(crate) const OIDC_CLIENT_ID: &str = "MARGINALIS_OIDC_CLIENT_ID";
pub(crate) const OIDC_CLIENT_SECRET: &str = "MARGINALIS_OIDC_CLIENT_SECRET";
pub(crate) const OIDC_CA_CERTIFICATE_FILE: &str = "MARGINALIS_OIDC_CA_CERTIFICATE_FILE";
pub(crate) const MCP_ENABLE: &str = "MARGINALIS_MCP_ENABLE";
pub(crate) const MCP_ALLOWED_ORIGINS: &str = "MARGINALIS_MCP_ALLOWED_ORIGINS";
pub(crate) const WEBHOOK_ALLOWED_HOSTS: &str = "MARGINALIS_WEBHOOK_ALLOWED_HOSTS";

/// 変数を設定する必要がある条件。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Requirement {
    /// 常に必要。
    Always,
    /// 省略できる。
    Optional,
    /// 設定するとMCPが有効になる。
    EnablesMcp,
}

/// 診断で値をどこまで出力するか。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Exposure {
    /// 値をそのまま出力する。
    Value,
    /// 設定の有無だけを出力する。秘密情報と、保存先の場所を含む値に使用する。
    Presence,
    /// カンマ区切りの要素数を出力する。
    ElementCount,
}

pub(crate) struct Variable {
    pub(crate) name: &'static str,
    pub(crate) requirement: Requirement,
    pub(crate) exposure: Exposure,
}

/// serviceが読み取るすべての環境変数。
pub(crate) const VARIABLES: &[Variable] = &[
    Variable {
        name: BASE_URL,
        requirement: Requirement::Always,
        exposure: Exposure::Value,
    },
    Variable {
        name: DATABASE_URL,
        requirement: Requirement::Always,
        exposure: Exposure::Presence,
    },
    Variable {
        name: LISTEN_ADDRESS,
        requirement: Requirement::Always,
        exposure: Exposure::Value,
    },
    Variable {
        name: OIDC_ISSUER_URL,
        requirement: Requirement::Always,
        exposure: Exposure::Value,
    },
    Variable {
        name: OIDC_CLIENT_ID,
        requirement: Requirement::Always,
        exposure: Exposure::Presence,
    },
    Variable {
        name: OIDC_CLIENT_SECRET,
        requirement: Requirement::Always,
        exposure: Exposure::Presence,
    },
    Variable {
        name: OIDC_CA_CERTIFICATE_FILE,
        requirement: Requirement::Optional,
        exposure: Exposure::Presence,
    },
    Variable {
        name: MCP_ENABLE,
        requirement: Requirement::EnablesMcp,
        exposure: Exposure::Value,
    },
    Variable {
        name: MCP_ALLOWED_ORIGINS,
        requirement: Requirement::Optional,
        exposure: Exposure::ElementCount,
    },
    Variable {
        name: WEBHOOK_ALLOWED_HOSTS,
        requirement: Requirement::Optional,
        exposure: Exposure::ElementCount,
    },
];

/// 設定済みの値を返す。空文字と空白だけの値、UTF-8として読めない値は未設定として扱う。
///
/// 設定構築と診断はこの関数だけを使い、「未設定」の判定を重複させない。
pub(crate) fn value(name: &str) -> Option<String> {
    env::var(name)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

/// カンマ区切りの値を、空要素を除いて分解する。
pub(crate) fn comma_separated(name: &str) -> Vec<String> {
    value(name)
        .map(|value| {
            value
                .split(',')
                .map(str::trim)
                .filter(|element| !element.is_empty())
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

/// MCPを有効にするかどうか。
///
/// Authorization Serverを内蔵したため、有効化に必要な外部設定がなくなった。判断材料は
/// `MARGINALIS_MCP_ENABLE`だけとし、`true`と`false`以外は誤設定として起動を止める。
/// 起動処理と診断はどちらもこの関数を使い、同じ入力に異なる判断をしない。
pub(crate) fn mcp_enabled() -> Result<bool, crate::config::ConfigurationError> {
    match value(MCP_ENABLE).as_deref() {
        None | Some("false") => Ok(false),
        Some("true") => Ok(true),
        Some(_) => Err(crate::config::ConfigurationError::InvalidMcpEnable),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn declared_variables_are_unique_and_namespaced() {
        let mut names = VARIABLES
            .iter()
            .map(|variable| variable.name)
            .collect::<Vec<_>>();
        names.sort_unstable();
        let unique = names.len();
        names.dedup();
        assert_eq!(names.len(), unique, "環境変数の宣言が重複しています");
        for name in names {
            assert!(
                name.starts_with("MARGINALIS_"),
                "環境変数はMARGINALIS_接頭辞を使用します: {name}"
            );
        }
    }

    #[test]
    fn exactly_one_variable_enables_mcp() {
        let enablers = VARIABLES
            .iter()
            .filter(|variable| variable.requirement == Requirement::EnablesMcp)
            .count();
        assert_eq!(enablers, 1);
    }
}
