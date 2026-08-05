//! AdocWeaveが解析した表を、文書間の対応検査で扱える行へ変換する。

use std::fs;
use std::path::PathBuf;

use adocweave::semantic::{SemanticNode, Table, walk};
use adocweave::{AnalysisOptions, Engine};

pub(crate) fn run(mut arguments: impl Iterator<Item = String>) -> Result<(), String> {
    if arguments.next().as_deref() != Some("--columns") {
        return Err(usage());
    }
    let columns = arguments
        .next()
        .ok_or_else(usage)?
        .parse::<usize>()
        .map_err(|_| "列数には正の整数を指定してください。".to_owned())?;
    if columns == 0 {
        return Err("列数には正の整数を指定してください。".to_owned());
    }
    if arguments.next().as_deref() != Some("--input") {
        return Err(usage());
    }
    let input = PathBuf::from(arguments.next().ok_or_else(usage)?);
    if arguments.next().is_some() {
        return Err(usage());
    }

    let source = fs::read_to_string(&input)
        .map_err(|error| format!("文書を読めません: {}: {error}", input.display()))?;
    for row in extract(&source, columns)? {
        println!("{}", row.join("\t"));
    }
    Ok(())
}

fn usage() -> String {
    "使用方法: marginalis-documentation extract-table-rows --columns COUNT --input DOCUMENT"
        .to_owned()
}

fn extract(source: &str, columns: usize) -> Result<Vec<Vec<String>>, String> {
    let analysis = Engine::new(AnalysisOptions::default())
        .analyze(source)
        .map_err(|error| format!("文書を解析できません: {error}"))?;
    let mut rows = Vec::new();
    let mut error = None;

    walk(analysis.document(), |node| {
        let SemanticNode::Table(table) = node else {
            return;
        };
        if error.is_some() {
            return;
        }
        match extract_table(table, columns) {
            Ok(table_rows) => rows.extend(table_rows),
            Err(message) => error = Some(message),
        }
    });

    error.map_or(Ok(rows), Err)
}

fn extract_table(table: &Table, columns: usize) -> Result<Vec<Vec<String>>, String> {
    let mut cells = Vec::new();
    for cell in table.rows.iter().flat_map(|row| &row.cells) {
        if cell.row_span != 1 || cell.column_span != 1 {
            return Err(
                "対応検査に使用する表では、複数行または複数列にまたがるセルを使用できません。"
                    .to_owned(),
            );
        }
        cells.push(normalize_cell(&cell.raw));
    }

    // `cols=",,"`のように幅を省略した列指定は、semantic modelでは個々の
    // セルとして保持される。呼び出し側が検査対象の列数を決め、表ごとに行を復元する。
    if cells.len() % columns != 0 {
        return Ok(Vec::new());
    }
    Ok(cells
        .chunks_exact(columns)
        .map(<[String]>::to_vec)
        .collect())
}

fn normalize_cell(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_only_tables_with_the_requested_column_count() {
        let source = concat!(
            "[cols=\",\"]\n|===\n|two\n|columns\n|===\n\n",
            "[cols=\",,\"]\n|===\n|id\n|verification\n|acceptance\n",
            "|REQ-TST-001\n|line one\nline two\n|https://example.invalid/evidence\n|===\n",
        );

        assert_eq!(
            extract(source, 3).expect("valid tables"),
            vec![
                vec!["id", "verification", "acceptance"],
                vec![
                    "REQ-TST-001",
                    "line one line two",
                    "https://example.invalid/evidence",
                ],
            ]
        );
    }

    #[test]
    fn escaped_separator_does_not_create_an_extra_cell() {
        let source = "[cols=\",\"]\n|===\n|C\\|C++\n|verification\n|===\n";

        assert_eq!(
            extract(source, 2).expect("escaped separator"),
            vec![vec!["C\\|C++", "verification"]]
        );
    }

    #[test]
    fn rejects_row_spans_that_cannot_be_represented_as_tsv() {
        let source = "[cols=\",\"]\n|===\n.2+|id\n|first\n|second\n|===\n";

        assert!(
            extract(source, 2)
                .expect_err("row span")
                .contains("複数行または複数列にまたがるセル")
        );
    }
}
