#!/usr/bin/env bash
set -euo pipefail

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
work_dir=$(mktemp -d)
trap 'rm -rf "$work_dir"' EXIT

cat >"$work_dir/expected" <<'EOF'
marginalis-application: marginalis-domain, mcp-authorization-server
marginalis-domain:
EOF
# 共有Authorization Serverは固定revisionのGit依存であり、path依存ではありません。
# それでも依存表へ現れることを確かめます。
cat >"$work_dir/good.json" <<'EOF'
{
  "workspace_members": ["domain", "application"],
  "packages": [
    {"id":"domain","name":"marginalis-domain","dependencies":[]},
    {"id":"application","name":"marginalis-application","dependencies":[
      {"name":"marginalis-domain","kind":null,"path":"/workspace/domain"},
      {"name":"mcp-authorization-server","kind":null,"path":null},
      {"name":"serde","kind":null,"path":null}
    ]}
  ]
}
EOF
bash "$script_dir/check-dependency-boundaries.sh" \
  "$work_dir/good.json" "$work_dir/expected"

for mutation in application-sqlx domain-reqwest; do
  case "$mutation" in
    application-sqlx)
      package=application
      dependency=sqlx
      ;;
    domain-reqwest)
      package=domain
      dependency=reqwest
      ;;
  esac
  jq \
    --arg package "$package" \
    --arg dependency "$dependency" \
    '(.packages[] | select(.id == $package) | .dependencies) +=
      [{"name":$dependency,"kind":null,"path":null}]' \
    "$work_dir/good.json" >"$work_dir/bad.json"
  if bash "$script_dir/check-dependency-boundaries.sh" \
    "$work_dir/bad.json" "$work_dir/expected" >/dev/null 2>&1; then
    echo "具象adapterへの依存を受理しました: $mutation" >&2
    exit 1
  fi
done

# 共有Authorization Serverが依存表から抜け落ちた場合を検出します。
jq '(.packages[] | select(.id == "application") | .dependencies) |=
  map(select(.name != "mcp-authorization-server"))' \
  "$work_dir/good.json" >"$work_dir/bad.json"
if bash "$script_dir/check-dependency-boundaries.sh" \
  "$work_dir/bad.json" "$work_dir/expected" >/dev/null 2>&1; then
  echo "共有Authorization Serverへの依存が消えた状態を受理しました" >&2
  exit 1
fi

# 共有Authorization Serverが別の層へ広がった場合を検出します。
jq '(.packages[] | select(.id == "domain") | .dependencies) +=
  [{"name":"mcp-authorization-server-cimd","kind":null,"path":null}]' \
  "$work_dir/good.json" >"$work_dir/bad.json"
if bash "$script_dir/check-dependency-boundaries.sh" \
  "$work_dir/bad.json" "$work_dir/expected" >/dev/null 2>&1; then
  echo "想定外の層への共有Authorization Server依存を受理しました" >&2
  exit 1
fi
