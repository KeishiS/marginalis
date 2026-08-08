//! Rust型のJSON Schemaから、frontendのTypeScript契約を生成する。
//!
//! 型定義と実行時検証の両方を[`crate::rest::component_schemas`]の出力から導出する。
//! HTTPクライアントは契約ではないため、frontend側の手書きmodule(`frontend/src/client.ts`)に置く。

use serde_json::{Map, Value};
use std::fmt::Write;

/// 生成するTypeScript契約の全文を返す。
pub fn typescript_contracts() -> String {
    let schemas = match crate::rest::component_schemas() {
        Value::Object(map) => map,
        _ => unreachable!("component schemas are a JSON object"),
    };
    let mut output = String::new();
    output.push_str(HEADER);
    let mut names: Vec<&String> = schemas.keys().collect();
    names.sort();
    for name in &names {
        let schema = &schemas[name.as_str()];
        output.push_str(&type_declaration(name, schema, &schemas));
    }
    output.push_str(TYPE_SUPPLEMENTS);
    output.push_str("export const CONTRACT_SCHEMAS: Record<string, unknown> = ");
    output.push_str(&serde_json::to_string_pretty(&Value::Object(schemas)).expect("schemas"));
    output.push_str(";\n");
    output.push_str(VALIDATOR);
    output.push_str(&parse_functions());
    output
}

const HEADER: &str = "\
// このファイルはmarginalis-contractから生成します。直接編集しないでください。
// 再生成: cargo run -p marginalis-contract --bin generate
/* eslint-disable */
";

/// schemaから導出できない、クライアント側の補足型。
const TYPE_SUPPLEMENTS: &str = "\
export type ValidationTarget = NoteValidationTarget;
/** サーバーの失敗応答。クライアントが合成する応答不正・通信失敗のcodeを含む。 */
export interface Problem {
  code: ProblemCode | \"invalid_response\" | \"network_error\";
  message: string;
  diagnostics?: NoteDiagnostic[];
}
";

/// 生成schemaのうち、TypeScriptの実行時検証で解釈する語彙だけを検査する検証器。
const VALIDATOR: &str = r#"
function resolveSchema(schema: unknown): unknown {
  if (
    typeof schema === "object" &&
    schema !== null &&
    "$ref" in schema &&
    typeof (schema as { $ref: unknown }).$ref === "string"
  ) {
    const name = (schema as { $ref: string }).$ref.split("/").pop() ?? "";
    const target = CONTRACT_SCHEMAS[name];
    if (target === undefined) throw new Error(`schema ${name} is missing`);
    return resolveSchema(target);
  }
  return schema;
}

function matchesType(value: unknown, type: string): boolean {
  switch (type) {
    case "object":
      return typeof value === "object" && value !== null && !Array.isArray(value);
    case "array":
      return Array.isArray(value);
    case "string":
      return typeof value === "string";
    case "integer":
      return Number.isSafeInteger(value);
    case "number":
      return typeof value === "number" && Number.isFinite(value);
    case "boolean":
      return typeof value === "boolean";
    case "null":
      return value === null;
    default:
      return true;
  }
}

function isValid(value: unknown, schema: unknown): boolean {
  try {
    assertValid(value, schema, "value");
    return true;
  } catch {
    return false;
  }
}

function assertValid(value: unknown, rawSchema: unknown, path: string): void {
  const schema = resolveSchema(rawSchema);
  if (schema === true || schema === undefined) return;
  if (schema === false) throw new Error(`${path} is invalid`);
  if (typeof schema !== "object" || schema === null) return;
  const s = schema as Record<string, unknown>;
  if (Array.isArray(s.allOf)) {
    for (const member of s.allOf) assertValid(value, member, path);
  }
  const alternatives = (s.oneOf ?? s.anyOf) as unknown[] | undefined;
  if (Array.isArray(alternatives)) {
    if (!alternatives.some((member) => isValid(value, member))) {
      throw new Error(`${path} is invalid`);
    }
  }
  if (s.type !== undefined) {
    const types = Array.isArray(s.type) ? s.type : [s.type];
    if (!types.some((type) => typeof type === "string" && matchesType(value, type))) {
      throw new Error(`${path} is invalid`);
    }
  }
  if (s.const !== undefined && JSON.stringify(value) !== JSON.stringify(s.const)) {
    throw new Error(`${path} is invalid`);
  }
  if (Array.isArray(s.enum)) {
    if (!s.enum.some((member) => JSON.stringify(member) === JSON.stringify(value))) {
      throw new Error(`${path} is invalid`);
    }
  }
  if (typeof value === "string") {
    // JSON Schemaの文字列長はUnicode code point数であり、JavaScriptのUTF-16 code unit数ではない。
    const characterLength = Array.from(value).length;
    if (typeof s.minLength === "number" && characterLength < s.minLength) {
      throw new Error(`${path} is invalid`);
    }
    if (typeof s.maxLength === "number" && characterLength > s.maxLength) {
      throw new Error(`${path} is invalid`);
    }
    if (typeof s.pattern === "string" && !new RegExp(s.pattern).test(value)) {
      throw new Error(`${path} is invalid`);
    }
  }
  if (typeof value === "number") {
    if (typeof s.minimum === "number" && value < s.minimum) {
      throw new Error(`${path} is invalid`);
    }
    if (typeof s.maximum === "number" && value > s.maximum) {
      throw new Error(`${path} is invalid`);
    }
  }
  if (Array.isArray(value)) {
    if (typeof s.minItems === "number" && value.length < s.minItems) {
      throw new Error(`${path} is invalid`);
    }
    if (typeof s.maxItems === "number" && value.length > s.maxItems) {
      throw new Error(`${path} is invalid`);
    }
    if (s.items !== undefined) {
      value.forEach((item, index) => assertValid(item, s.items, `${path}[${index}]`));
    }
  }
  if (typeof value === "object" && value !== null && !Array.isArray(value)) {
    const record = value as Record<string, unknown>;
    const properties = (s.properties ?? {}) as Record<string, unknown>;
    if (Array.isArray(s.required)) {
      for (const name of s.required) {
        if (typeof name === "string" && record[name] === undefined) {
          throw new Error(`${path}.${name} is missing`);
        }
      }
    }
    for (const [name, property] of Object.entries(properties)) {
      if (record[name] !== undefined) {
        assertValid(record[name], property, `${path}.${name}`);
      }
    }
    if (s.additionalProperties === false && s.properties !== undefined) {
      for (const name of Object.keys(record)) {
        if (!(name in properties)) {
          throw new Error(`${path}.${name} is not allowed`);
        }
      }
    }
  }
}

function parseAs<T>(value: unknown, schemaName: string, label: string): T {
  assertValid(value, CONTRACT_SCHEMAS[schemaName], label);
  return value as T;
}

function parseArrayAs<T>(value: unknown, schemaName: string, label: string): T[] {
  if (!Array.isArray(value)) throw new Error(`${label} are invalid`);
  return value.map((item, index) => parseAs<T>(item, schemaName, `${label}[${index}]`));
}
"#;

/// 生成するparse関数の一覧。`(関数名, schema名, TypeScript型, ラベル, 配列か)`。
const PARSERS: &[(&str, &str, &str, &str, bool)] = &[
    (
        "parseApplicationConfig",
        "ApplicationConfig",
        "ApplicationConfig",
        "application config",
        false,
    ),
    ("parseNote", "Note", "Note", "note", false),
    (
        "parseNoteSummary",
        "NoteSummary",
        "NoteSummary",
        "note summary",
        false,
    ),
    (
        "parseNoteSummaries",
        "NoteSummary",
        "NoteSummary",
        "note summaries",
        true,
    ),
    (
        "parseNoteListEntry",
        "NoteListEntry",
        "NoteListEntry",
        "note list entry",
        false,
    ),
    (
        "parseNoteListEntries",
        "NoteListEntry",
        "NoteListEntry",
        "note list entries",
        true,
    ),
    (
        "parseDeletedNoteListEntries",
        "DeletedNoteListEntry",
        "DeletedNoteListEntry",
        "deleted note list entries",
        true,
    ),
    (
        "parseNoteReview",
        "NoteReview",
        "NoteReview",
        "note review",
        false,
    ),
    (
        "parseNoteGraph",
        "NoteGraph",
        "NoteGraph",
        "note graph",
        false,
    ),
    ("parseNoteView", "NoteView", "NoteView", "note view", false),
    ("parseNoteAcl", "NoteAcl", "NoteAcl", "note ACL", false),
    (
        "parseNotePreview",
        "NotePreview",
        "NotePreview",
        "note preview",
        false,
    ),
    ("parseProblem", "Problem", "Problem", "problem", false),
    (
        "parseMathMacroSettings",
        "MathMacroSettings",
        "MathMacroSettings",
        "math macro settings",
        false,
    ),
    (
        "parseMcpScopeCeiling",
        "McpScopeCeiling",
        "McpScopeCeiling",
        "MCP scope ceiling",
        false,
    ),
    (
        "parseMcpClientAuthorization",
        "McpClientAuthorization",
        "McpClientAuthorization",
        "MCP client authorization",
        false,
    ),
    (
        "parseMcpClientAuthorizations",
        "McpClientAuthorization",
        "McpClientAuthorization",
        "MCP client authorizations",
        true,
    ),
    (
        "parseBibliographyItem",
        "BibliographyItem",
        "BibliographyItem",
        "bibliography item",
        false,
    ),
    (
        "parseBibliographyItems",
        "BibliographyItem",
        "BibliographyItem",
        "bibliography items",
        true,
    ),
    (
        "parseBibliographyImportSources",
        "BibliographyImportSource",
        "BibliographyImportSource",
        "bibliography import sources",
        true,
    ),
    (
        "parseBibliographyImportPreview",
        "BibliographyImportPreview",
        "BibliographyImportPreview",
        "bibliography import preview",
        false,
    ),
    (
        "parseBibliographyImportResult",
        "BibliographyImportResult",
        "BibliographyImportResult",
        "bibliography import result",
        false,
    ),
];

fn parse_functions() -> String {
    let mut output = String::new();
    for (function, schema, ts_type, label, is_array) in PARSERS {
        if *is_array {
            let _ = writeln!(
                output,
                "export function {function}(value: unknown): {ts_type}[] {{\n  return parseArrayAs<{ts_type}>(value, \"{schema}\", \"{label}\");\n}}"
            );
        } else {
            let _ = writeln!(
                output,
                "export function {function}(value: unknown): {ts_type} {{\n  return parseAs<{ts_type}>(value, \"{schema}\", \"{label}\");\n}}"
            );
        }
    }
    output
}

/// 1つの名前付きschemaをTypeScriptの型宣言にする。
fn type_declaration(name: &str, schema: &Value, definitions: &Map<String, Value>) -> String {
    // Problemはクライアント補足型として別途出力する。
    if name == "Problem" {
        return String::new();
    }
    if is_plain_object(schema) {
        let mut output = format!("export interface {name} {{\n");
        output.push_str(&object_members(schema, definitions, "  "));
        output.push_str("}\n");
        output
    } else {
        format!(
            "export type {name} = {};\n",
            type_expression(schema, definitions)
        )
    }
}

fn is_plain_object(schema: &Value) -> bool {
    schema.get("type").and_then(Value::as_str) == Some("object")
        && schema.get("properties").is_some()
        && schema.get("oneOf").is_none()
        && schema.get("anyOf").is_none()
        && schema.get("allOf").is_none()
}

fn object_members(schema: &Value, definitions: &Map<String, Value>, indent: &str) -> String {
    let empty = Map::new();
    let properties = schema
        .get("properties")
        .and_then(Value::as_object)
        .unwrap_or(&empty);
    let required: Vec<&str> = schema
        .get("required")
        .and_then(Value::as_array)
        .map(|values| values.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let mut output = String::new();
    for (property, subschema) in properties {
        let optional = if required.contains(&property.as_str()) {
            ""
        } else {
            "?"
        };
        let _ = writeln!(
            output,
            "{indent}{}{optional}: {};",
            member_name(property),
            type_expression(subschema, definitions)
        );
    }
    output
}

fn member_name(name: &str) -> String {
    if name
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || character == '_' || character == '$')
        && !name.chars().next().is_some_and(|c| c.is_ascii_digit())
    {
        name.to_owned()
    } else {
        format!("\"{name}\"")
    }
}

/// schemaをTypeScriptの型表現にする。
fn type_expression(schema: &Value, definitions: &Map<String, Value>) -> String {
    match schema {
        Value::Bool(true) => "unknown".to_owned(),
        Value::Bool(false) => "never".to_owned(),
        Value::Object(map) => object_type_expression(map, definitions),
        _ => "unknown".to_owned(),
    }
}

fn object_type_expression(map: &Map<String, Value>, definitions: &Map<String, Value>) -> String {
    if let Some(reference) = map.get("$ref").and_then(Value::as_str) {
        return reference.rsplit('/').next().unwrap_or("unknown").to_owned();
    }
    if let Some(constant) = map.get("const") {
        return literal(constant);
    }
    if let Some(members) = map.get("enum").and_then(Value::as_array) {
        return union(members.iter().map(literal));
    }
    if let Some(members) = map.get("allOf").and_then(Value::as_array) {
        let parts: Vec<String> = members
            .iter()
            .map(|member| type_expression(member, definitions))
            .collect();
        let mut expression = parts.join(" & ");
        if map.get("properties").is_some() {
            expression.push_str(" & { ");
            expression.push_str(
                object_members(&Value::Object(map.clone()), definitions, "")
                    .trim_end()
                    .replace('\n', " ")
                    .as_str(),
            );
            expression.push_str(" }");
        }
        return expression;
    }
    if let Some(members) = map
        .get("oneOf")
        .or_else(|| map.get("anyOf"))
        .and_then(Value::as_array)
    {
        return union(
            members
                .iter()
                .map(|member| type_expression(member, definitions)),
        );
    }
    let types: Vec<&str> = match map.get("type") {
        Some(Value::String(single)) => vec![single.as_str()],
        Some(Value::Array(list)) => list.iter().filter_map(Value::as_str).collect(),
        _ => Vec::new(),
    };
    if types.is_empty() {
        return "unknown".to_owned();
    }
    union(
        types
            .iter()
            .map(|kind| single_type_expression(kind, map, definitions)),
    )
}

fn single_type_expression(
    kind: &str,
    map: &Map<String, Value>,
    definitions: &Map<String, Value>,
) -> String {
    match kind {
        "string" => "string".to_owned(),
        "integer" | "number" => "number".to_owned(),
        "boolean" => "boolean".to_owned(),
        "null" => "null".to_owned(),
        "array" => {
            let items = map
                .get("items")
                .map(|items| type_expression(items, definitions))
                .unwrap_or_else(|| "unknown".to_owned());
            if items.contains(' ') {
                format!("({items})[]")
            } else {
                format!("{items}[]")
            }
        }
        "object" => {
            if map.get("properties").is_some() {
                let members = object_members(&Value::Object(map.clone()), definitions, "")
                    .trim_end()
                    .replace('\n', " ");
                format!("{{ {members} }}")
            } else {
                "Record<string, unknown>".to_owned()
            }
        }
        _ => "unknown".to_owned(),
    }
}

fn union(members: impl Iterator<Item = String>) -> String {
    let parts: Vec<String> = members.collect();
    parts.join(" | ")
}

fn literal(value: &Value) -> String {
    match value {
        Value::String(text) => format!("\"{text}\""),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_typescript_exports_every_contract_parser() {
        let output = typescript_contracts();
        for (function, ..) in PARSERS {
            assert!(
                output.contains(&format!("export function {function}(")),
                "{function} is missing"
            );
        }
        assert!(output.contains("export interface Note {"));
        assert!(output.contains("export interface Problem {"));
        assert!(output.contains("export const CONTRACT_SCHEMAS"));
    }
}
