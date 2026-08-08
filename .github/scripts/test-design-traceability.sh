#!/usr/bin/env bash
set -euo pipefail

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
work_dir=$(mktemp -d)
trap 'rm -rf "$work_dir"' EXIT

write_valid_architecture() {
  cat >"$work_dir/architecture.adoc" <<'EOF'
== 一貫して満たすべき設計条件

* *ARCH-TST-001 — 試験条件*: 検証できること。

== 次の節
EOF
}

write_valid_traceability() {
  cat >"$work_dir/traceability.adoc" <<'EOF'
[cols="1,1"]
|===
|設計条件ID
|主な自動検証
|ARCH-TST-001
|``fixture-test``
|===
EOF
}

write_valid_architecture
write_valid_traceability
bash "$script_dir/check-design-traceability.sh" \
  "$work_dir/architecture.adoc" "$work_dir/traceability.adoc"

cat >"$work_dir/architecture.adoc" <<'EOF'
== 一貫して満たすべき設計条件

* 識別子のない設計条件。
EOF
if bash "$script_dir/check-design-traceability.sh" \
  "$work_dir/architecture.adoc" "$work_dir/traceability.adoc" >/dev/null 2>&1; then
  echo "識別子のない設計条件を受理しました。" >&2
  exit 1
fi

write_valid_architecture
cat >"$work_dir/traceability.adoc" <<'EOF'
[cols="1,1"]
|===
|設計条件ID
|主な自動検証
|ARCH-TST-001
|
|===
EOF
if bash "$script_dir/check-design-traceability.sh" \
  "$work_dir/architecture.adoc" "$work_dir/traceability.adoc" >/dev/null 2>&1; then
  echo "検証方法が空の設計条件を受理しました。" >&2
  exit 1
fi

write_valid_traceability
cat >"$work_dir/architecture.adoc" <<'EOF'
== 一貫して満たすべき設計条件

* *ARCH-TST-001 — 一つ目*: 検証できること。
* *ARCH-TST-001 — 二つ目*: 同じ識別子を持つこと。
EOF
if bash "$script_dir/check-design-traceability.sh" \
  "$work_dir/architecture.adoc" "$work_dir/traceability.adoc" >/dev/null 2>&1; then
  echo "重複した設計条件IDを受理しました。" >&2
  exit 1
fi

write_valid_architecture
cat >"$work_dir/traceability.adoc" <<'EOF'
[cols="1,1"]
|===
|設計条件ID
|主な自動検証
|ARCH-TST-001
|``fixture-test``
|ARCH-TST-001
|``duplicate-test``
|===
EOF
if bash "$script_dir/check-design-traceability.sh" \
  "$work_dir/architecture.adoc" "$work_dir/traceability.adoc" >/dev/null 2>&1; then
  echo "対応表で重複した設計条件IDを受理しました。" >&2
  exit 1
fi

cat >"$work_dir/traceability.adoc" <<'EOF'
[cols="1,1"]
|===
|設計条件ID
|主な自動検証
|ARCH-OLD-001
|``stale-test``
|===
EOF
if bash "$script_dir/check-design-traceability.sh" \
  "$work_dir/architecture.adoc" "$work_dir/traceability.adoc" >/dev/null 2>&1; then
  echo "設計条件と一致しない対応表を受理しました。" >&2
  exit 1
fi
