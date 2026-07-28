#!/usr/bin/env bash
set -euo pipefail

status=0
temporary_directory=$(mktemp -d)
trap 'rm -rf "$temporary_directory"' EXIT

requirements=docs/requirements.md
traceability=docs/traceability.md
requirement_ids="$temporary_directory/requirement-ids"
traceability_ids="$temporary_directory/traceability-ids"
grep -oE 'REQ-[A-Z]+-[0-9]{3}' "$requirements" | sort >"$requirement_ids"
grep -oE 'REQ-[A-Z]+-[0-9]{3}' "$traceability" | sort >"$traceability_ids"

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
done < <(find docs/acceptance-results -type f -name '*.md' -print0 | sort -z)

exit "$status"
