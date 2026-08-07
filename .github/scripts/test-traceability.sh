#!/usr/bin/env bash
set -euo pipefail

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
work_dir=$(mktemp -d)
trap 'rm -rf "$work_dir"' EXIT

cat >"$work_dir/requirements.adoc" <<'EOF'
* *REQ-TST-001 — 試験要件*: 検証できること。
EOF
cat >"$work_dir/traceability.adoc" <<'EOF'
[cols="1,1,1"]
|===
|要件
|自動検証
|受入
|REQ-TST-001
|``fixture-test``
|手動確認
|===
EOF
bash "$script_dir/check-traceability.sh" \
  "$work_dir/requirements.adoc" "$work_dir/traceability.adoc"

cat >"$work_dir/traceability.adoc" <<'EOF'
[cols="1,1,1"]
|===
|要件
|自動検証
|受入
|REQ-TST-001
|
|手動確認
|===
EOF
if bash "$script_dir/check-traceability.sh" \
  "$work_dir/requirements.adoc" "$work_dir/traceability.adoc" \
  >/dev/null 2>&1; then
  echo "検証方法が空の対応表を受理しました。" >&2
  exit 1
fi

cat >"$work_dir/traceability.adoc" <<'EOF'
[cols="1,1,1"]
|===
|要件
|自動検証
|受入
|REQ-TST-001
|``fixture-test``
|手動確認
|REQ-TST-001
|``duplicate-test``
|手動確認
|===
EOF
if bash "$script_dir/check-traceability.sh" \
  "$work_dir/requirements.adoc" "$work_dir/traceability.adoc" \
  >/dev/null 2>&1; then
  echo "要件IDが重複する対応表を受理しました。" >&2
  exit 1
fi
