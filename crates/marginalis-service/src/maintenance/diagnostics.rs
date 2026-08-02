//! SQLiteと公開設定の読み取り専用診断。

use std::collections::BTreeMap;

use marginalis_sqlite::SqliteDatabase;

use crate::environment::{self, Exposure, Requirement};

#[derive(serde::Serialize)]
struct DiagnosticReport {
    status: &'static str,
    event: &'static str,
    database: marginalis_sqlite::SqliteDiagnosticReport,
    configuration: PublicConfigurationReport,
}

/// 環境変数の宣言から導出する公開設定の報告。
///
/// 変数ごとの項目は[`environment::VARIABLES`]を走査して作るため、変数を追加しても報告漏れが
/// 起きない。「未設定」の判定も起動処理と同じ関数を使う。
#[derive(serde::Serialize)]
struct PublicConfigurationReport {
    mcp_enabled: bool,
    variables: BTreeMap<&'static str, VariableReport>,
}

#[derive(serde::Serialize)]
struct VariableReport {
    set: bool,
    required: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    value: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    element_count: Option<usize>,
}

/// SQLiteと公開設定を変更せずに検査し、結果をJSONで出力する。
pub(crate) async fn diagnose() -> Result<(), Box<dyn std::error::Error>> {
    let database_url = environment::value(environment::DATABASE_URL);
    let database = match database_url.as_deref() {
        Some(database_url) => SqliteDatabase::diagnose(database_url).await,
        None => SqliteDatabase::diagnose("sqlite://configuration-is-missing?mode=ro").await,
    };
    let healthy = database.healthy();
    let report = DiagnosticReport {
        status: if healthy { "ok" } else { "failed" },
        event: "diagnostics.completed",
        database,
        configuration: public_configuration(),
    };
    serde_json::to_writer(std::io::stdout().lock(), &report)?;
    println!();
    if healthy {
        Ok(())
    } else {
        Err("diagnostics reported an unhealthy database".into())
    }
}

fn public_configuration() -> PublicConfigurationReport {
    let mcp_enabled = environment::mcp_enabled().unwrap_or(false);
    let variables = environment::VARIABLES
        .iter()
        .map(|variable| {
            let value = environment::value(variable.name);
            let report = VariableReport {
                set: value.is_some(),
                required: match variable.requirement {
                    Requirement::Always => true,
                    Requirement::Optional | Requirement::EnablesMcp => false,
                },
                value: match variable.exposure {
                    Exposure::Value => value,
                    Exposure::Presence | Exposure::ElementCount => None,
                },
                element_count: match variable.exposure {
                    Exposure::ElementCount => {
                        Some(environment::comma_separated(variable.name).len())
                    }
                    Exposure::Value | Exposure::Presence => None,
                },
            };
            (variable.name, report)
        })
        .collect();
    PublicConfigurationReport {
        mcp_enabled,
        variables,
    }
}
