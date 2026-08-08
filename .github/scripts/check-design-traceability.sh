#!/usr/bin/env bash
set -euo pipefail

status=0
temporary_directory=$(mktemp -d)
trap 'rm -rf "$temporary_directory"' EXIT

architecture="${1:-docs/developer-guide/architecture.adoc}"
traceability="${2:-docs/developer-guide/traceability.adoc}"
architecture_ids="$temporary_directory/architecture-ids"
traceability_rows="$temporary_directory/traceability-rows"
traceability_ids="$temporary_directory/traceability-ids"

if ! awk '
  /^== 一貫して満たすべき設計条件[[:space:]]*$/ {
    in_conditions = 1
    next
  }
  in_conditions && /^== / {
    in_conditions = 0
  }
  in_conditions && /^\* / {
    condition_count++
    if ($0 !~ /^\* \*ARCH-[A-Z]+-[0-9]{3} —/) {
      print "設計条件に有効な識別子がありません: " $0 > "/dev/stderr"
      invalid = 1
      next
    }
    identifier = $0
    sub(/^\* \*/, "", identifier)
    sub(/ .*/, "", identifier)
    print identifier
  }
  END {
    if (condition_count == 0) {
      print "アーキテクチャに設計条件がありません。" > "/dev/stderr"
      invalid = 1
    }
    exit invalid
  }
' "$architecture" >"$architecture_ids"; then
  status=1
fi
sort -o "$architecture_ids" "$architecture_ids"

if ! cargo run --quiet --locked -p marginalis-documentation -- \
  extract-table-rows --columns 2 --input "$traceability" |
  awk -F '\t' '
  $1 ~ /^ARCH-[A-Z]+-[0-9]{3}$/ {
    identifier = $1
    verification = $2
    if (verification == "") {
      print "設計条件と検証の対応表に検証方法がありません: " identifier > "/dev/stderr"
      invalid = 1
    }
    print identifier "\t" verification
  }
  END { exit invalid }
' >"$traceability_rows"; then
  status=1
fi
cut -f1 "$traceability_rows" | sort >"$traceability_ids"

if [[ -n "$(uniq -d "$architecture_ids")" ]]; then
  echo "アーキテクチャの設計条件IDが重複しています。" >&2
  uniq -d "$architecture_ids" >&2
  status=1
fi
if [[ -n "$(uniq -d "$traceability_ids")" ]]; then
  echo "設計条件と検証の対応表に設計条件IDが重複しています。" >&2
  uniq -d "$traceability_ids" >&2
  status=1
fi
if ! diff -u "$architecture_ids" "$traceability_ids"; then
  echo "アーキテクチャと設計条件・検証対応表のIDが一致しません。" >&2
  status=1
fi

if [[ "$architecture" == "docs/developer-guide/architecture.adoc" && "$traceability" == "docs/developer-guide/traceability.adoc" ]]; then
  session_row=$(grep -E '^ARCH-SESSION-001[[:space:]]' "$traceability_rows" || true)
  regression_test='issuing_login_attempt_reclaims_expired_capacity_before_enforcing_the_limit'
  if [[ "$session_row" != *"$regression_test"* ]]; then
    echo "ARCH-SESSION-001にlogin attempt上限の回帰試験がありません。" >&2
    status=1
  fi
  if ! grep -Fq "async fn $regression_test" crates/marginalis-sqlite/src/tests/sessions.rs; then
    echo "login attempt上限の回帰試験が実装にありません。" >&2
    status=1
  fi
fi

exit "$status"
