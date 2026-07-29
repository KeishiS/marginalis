use std::{env, fs, path::PathBuf};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let outputs = [
        (
            root.join("docs/openapi.json"),
            format!(
                "{}\n",
                serde_json::to_string_pretty(&marginalis_contract::openapi_document())?
            ),
        ),
        (
            root.join("docs/mcp-tools.json"),
            format!(
                "{}\n",
                serde_json::to_string_pretty(&marginalis_contract::mcp_tool_contracts())?
            ),
        ),
        (
            root.join("frontend/src/generated/contracts.ts"),
            marginalis_contract::typescript_contracts().to_owned(),
        ),
    ];
    let check = env::args().any(|argument| argument == "--check");
    for (path, content) in outputs {
        if check {
            if fs::read_to_string(&path).ok().as_deref() != Some(&content) {
                return Err(format!("generated contract is stale: {}", path.display()).into());
            }
        } else {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(path, content)?;
        }
    }
    Ok(())
}
