//! 本アプリ向けのAdocWeave統合境界。
//!
//! このcrateはAdocWeaveの公開APIだけに依存し、アプリ固有のプロファイル、参照解決、
//! 描画ポリシーを段階的に追加する。

use core::fmt;

/// 採用したAdocWeaveソースcommit。
pub const ADOCWEAVE_SOURCE_REVISION: &str = "72ad5a677e179448b4de7f524710f4e455aa163d";

/// アプリが受理するAdocWeave公開契約の組。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AdocWeaveContracts {
    pub core_profile: u16,
    pub core_api: u16,
    pub html: u16,
    pub projection: u16,
    pub conformance: u16,
    pub wasm_api: u16,
}

/// 現在固定しているAdocWeaveリリース契約。
pub const PINNED_CONTRACTS: AdocWeaveContracts = AdocWeaveContracts {
    core_profile: 1,
    core_api: 1,
    html: 1,
    projection: 1,
    conformance: 1,
    wasm_api: 1,
};

/// 実際にlinkされたAdocWeaveの契約。
pub const fn runtime_contracts() -> AdocWeaveContracts {
    AdocWeaveContracts {
        core_profile: adocweave::CORE_PROFILE_VERSION,
        core_api: adocweave::CORE_API_VERSION,
        html: adocweave::html::HTML_CONTRACT_VERSION,
        projection: adocweave::projection::PROJECTION_CONTRACT_VERSION,
        conformance: adocweave::conformance::CONFORMANCE_CONTRACT_VERSION,
        wasm_api: adocweave_wasm::WASM_API_VERSION,
    }
}

/// 固定した契約と実行時の契約が異なる場合に返すエラー。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ContractMismatch {
    pub expected: AdocWeaveContracts,
    pub actual: AdocWeaveContracts,
}

impl fmt::Display for ContractMismatch {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "AdocWeave contract mismatch: expected {:?}, got {:?}",
            self.expected, self.actual
        )
    }
}

impl std::error::Error for ContractMismatch {}

/// linkされた依存が、本アプリの固定した公開契約と一致することを検証する。
pub fn verify_runtime_contracts() -> Result<(), ContractMismatch> {
    let actual = runtime_contracts();
    if actual == PINNED_CONTRACTS {
        Ok(())
    } else {
        Err(ContractMismatch {
            expected: PINNED_CONTRACTS,
            actual,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{ADOCWEAVE_SOURCE_REVISION, PINNED_CONTRACTS, verify_runtime_contracts};

    #[test]
    fn pinned_revision_is_a_full_git_object_id() {
        assert_eq!(ADOCWEAVE_SOURCE_REVISION.len(), 40);
        assert!(
            ADOCWEAVE_SOURCE_REVISION
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
        );
    }

    #[test]
    fn linked_contracts_match_the_pinned_contracts() {
        assert_eq!(PINNED_CONTRACTS.core_profile, 1);
        verify_runtime_contracts().expect("pinned AdocWeave contracts must match");
    }
}
