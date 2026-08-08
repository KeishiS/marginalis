#!/usr/bin/env bash
set -euo pipefail

status=0
temporary_directory=$(mktemp -d)
trap 'rm -rf "$temporary_directory"' EXIT

requirements="${1:-docs/developer-guide/requirements.adoc}"
traceability="${2:-docs/developer-guide/traceability.adoc}"
requirement_ids="$temporary_directory/requirement-ids"
traceability_rows="$temporary_directory/traceability-rows"
traceability_ids="$temporary_directory/traceability-ids"

grep -oE 'REQ-[A-Z]+-[0-9]{3}' "$requirements" | sort >"$requirement_ids"
if ! cargo run --quiet --locked -p marginalis-documentation -- \
  extract-table-rows --columns 3 --input "$traceability" |
  awk -F '\t' '
  $1 ~ /^REQ-[A-Z]+-[0-9]{3}$/ {
    id = $1
    verification = $2
    acceptance = $3
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
' >"$traceability_rows"; then
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
exit "$status"
