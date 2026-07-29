#!/usr/bin/env bash
set -euo pipefail

status=0
temporary_directory=$(mktemp -d)
trap 'rm -rf "$temporary_directory"' EXIT

requirements="${1:-docs/requirements.md}"
traceability="${2:-docs/traceability.md}"
acceptance_directory="${3:-docs/acceptance-results}"
requirement_ids="$temporary_directory/requirement-ids"
traceability_rows="$temporary_directory/traceability-rows"
traceability_ids="$temporary_directory/traceability-ids"

grep -oE 'REQ-[A-Z]+-[0-9]{3}' "$requirements" | sort >"$requirement_ids"
if ! awk -F '|' '
  function trim(value) {
    sub(/^[[:space:]]+/, "", value)
    sub(/[[:space:]]+$/, "", value)
    return value
  }
  /^\|[[:space:]]*REQ-[A-Z]+-[0-9]{3}[[:space:]]*\|/ {
    id = trim($2)
    verification = trim($3)
    acceptance = trim($4)
    if (verification == "") {
      print "要件と検証の対応表に検証方法がありません: " id > "/dev/stderr"
      invalid = 1
    }
    if (acceptance == "") {
      print "要件と検証の対応表に受入方法がありません: " id > "/dev/stderr"
      invalid = 1
    }
    print id "\t" verification "\t" acceptance
  }
  END { exit invalid }
' "$traceability" >"$traceability_rows"; then
  status=1
fi
cut -f1 "$traceability_rows" | sort >"$traceability_ids"

if [[ -n "$(uniq -d "$requirement_ids")" ]]; then
  echo "現行要件の要件IDが重複しています。" >&2
  uniq -d "$requirement_ids" >&2
  status=1
fi
if [[ -n "$(uniq -d "$traceability_ids")" ]]; then
  echo "要件と検証の対応表に要件IDが重複しています。" >&2
  uniq -d "$traceability_ids" >&2
  status=1
fi
if ! diff -u "$requirement_ids" "$traceability_ids"; then
  echo "現行要件と要件・検証対応表のIDが一致しません。" >&2
  status=1
fi
if [[ "$requirements" == "docs/requirements.md" && "$traceability" == "docs/traceability.md" ]]; then
  operations_row=$(grep -E '^\|[[:space:]]*REQ-OPS-007[[:space:]]*\|' "$traceability" || true)
  for reference in \
    'observability-check' \
    'observability_logs_safe_http_and_mcp_results' \
    'kanidm-discovery-vm'; do
    if [[ "$operations_row" != *"$reference"* ]]; then
      echo "REQ-OPS-007の検証参照がありません: $reference" >&2
      status=1
    fi
  done
  grep -Fq '[tasks.observability-check]' Makefile.toml || status=1
  rg -q 'fn observability_logs_safe_http_and_mcp_results' crates/marginalis-web/src/http/tests.rs ||
    status=1
  rg -q 'kanidm-discovery-vm' flake.nix || status=1
fi

if [[ -d "$acceptance_directory" ]]; then
  while IFS= read -r -d '' result; do
    if ! grep -Fq '## 対象' "$result" || ! grep -Fq '## 結果' "$result"; then
      echo "版別受入結果に対象または結果がありません: $result" >&2
      status=1
    fi
    while IFS= read -r row; do
      if [[ "$row" != *"]("* ]]; then
        echo "成功した受入結果に証跡リンクがありません: $result: $row" >&2
        status=1
      fi
    done < <(grep -E '^\|[^|]+\|[[:space:]]*成功[[:space:]]*\|' "$result" || true)
  done < <(find "$acceptance_directory" -type f -name '*.md' -print0 | sort -z)
fi

exit "$status"
