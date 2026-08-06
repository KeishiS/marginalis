#!/usr/bin/env bash
set -euo pipefail

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
work_dir=$(mktemp -d)
trap 'rm -rf "$work_dir"' EXIT

revision=1111111111111111111111111111111111111111
other_revision=2222222222222222222222222222222222222222
declaration="git+https://github.com/KeishiS/mcp-authorization-server.git?rev=$revision"
package_source="$declaration#$revision"

jq -n \
  --arg declaration "$declaration" \
  --arg package_source "$package_source" '
{
  workspace_members: ["service", "sqlite"],
  packages: [
    {
      id: "service", name: "marginalis-service", version: "0.34.0",
      source: null, license: "MIT OR Apache-2.0",
      dependencies: [
        {name: "marginalis-sqlite", kind: null, req: "*", source: null, path: "/w/sqlite"},
        {name: "mcp-authorization-server-cimd", kind: null, req: "=0.1.0", source: $declaration}
      ]
    },
    {
      id: "sqlite", name: "marginalis-sqlite", version: "0.34.0",
      source: null, license: "MIT OR Apache-2.0",
      dependencies: [
        {name: "mcp-authorization-server", kind: null, req: "=0.1.0", source: $declaration},
        {name: "mcp-authorization-server", kind: "dev", req: "=0.1.0", source: $declaration,
         features: ["testkit"]}
      ]
    },
    {
      id: "core", name: "mcp-authorization-server", version: "0.1.0",
      source: $package_source, license: "MIT OR Apache-2.0", dependencies: []
    },
    {
      id: "cimd", name: "mcp-authorization-server-cimd", version: "0.1.0",
      source: $package_source, license: "MIT OR Apache-2.0", dependencies: []
    }
  ],
  resolve: {
    nodes: [
      {id: "service", deps: [
        {pkg: "sqlite", dep_kinds: [{kind: null}]},
        {pkg: "cimd", dep_kinds: [{kind: null}]}
      ]},
      {id: "sqlite", deps: [{pkg: "core", dep_kinds: [{kind: null}]}]},
      {id: "cimd", deps: [{pkg: "core", dep_kinds: [{kind: null}]}]},
      {id: "core", deps: []}
    ]
  }
}' >"$work_dir/good.json"

bash "$script_dir/check-shared-authorization-server.sh" "$work_dir/good.json" >/dev/null

reject() {
  local description=$1
  if bash "$script_dir/check-shared-authorization-server.sh" \
    "$work_dir/bad.json" >/dev/null 2>&1; then
    echo "受理してはいけない状態を受理しました: $description" >&2
    exit 1
  fi
}

# 版を範囲指定へ緩めた場合
jq '(.packages[] | select(.id == "sqlite") | .dependencies[0] | .req) = "^0.1"' \
  "$work_dir/good.json" >"$work_dir/bad.json"
reject "版の範囲指定"

# 2 crateが別のcommitを指す場合
jq --arg source "git+https://github.com/KeishiS/mcp-authorization-server.git?rev=$other_revision" \
  '(.packages[] | select(.id == "service") | .dependencies[1] | .source) = $source' \
  "$work_dir/good.json" >"$work_dir/bad.json"
reject "revisionの不一致"

# revisionが短縮形の場合
jq --arg source "git+https://github.com/KeishiS/mcp-authorization-server.git?rev=1111111" \
  '(.packages[] | select(.id == "sqlite") | .dependencies[0] | .source) = $source' \
  "$work_dir/good.json" >"$work_dir/bad.json"
reject "短縮revision"

# 別リポジトリを指す場合
jq --arg source "git+https://example.test/mcp-authorization-server.git?rev=$revision" \
  '(.packages[] | select(.id == "sqlite") | .dependencies[0] | .source) = $source' \
  "$work_dir/good.json" >"$work_dir/bad.json"
reject "別リポジトリ"

# ローカルpathから解決された場合
jq '(.packages[] | select(.id == "core") | .source) = null' \
  "$work_dir/good.json" >"$work_dir/bad.json"
reject "ローカルpath版"

# 同じcrateが複数版存在する場合
jq --arg package_source "$package_source" '
  .packages += [{
    id: "core-old", name: "mcp-authorization-server", version: "0.0.1",
    source: $package_source, license: "MIT OR Apache-2.0", dependencies: []
  }]' "$work_dir/good.json" >"$work_dir/bad.json"
reject "複数版の同居"

# production依存でtestkitを有効にした場合
jq '(.packages[] | select(.id == "sqlite") | .dependencies[0] | .features) = ["testkit"]' \
  "$work_dir/good.json" >"$work_dir/bad.json"
reject "production依存のtestkit"

# ライセンスが変わった場合
jq '(.packages[] | select(.id == "cimd") | .license) = "GPL-3.0"' \
  "$work_dir/good.json" >"$work_dir/bad.json"
reject "想定外のライセンス"

# CIMDが中核へ依存しなくなった場合
jq '(.resolve.nodes[] | select(.id == "cimd") | .deps) = []' \
  "$work_dir/good.json" >"$work_dir/bad.json"
reject "CIMDから中核への依存欠落"

# 本番依存から到達できなくなった場合
jq '(.resolve.nodes[] | select(.id == "service") | .deps[1] | .dep_kinds) = [{kind: "dev"}]' \
  "$work_dir/good.json" >"$work_dir/bad.json"
reject "本番依存からの到達不能"

# 依存宣言そのものが消えた場合
jq '[.packages[] | .dependencies |= map(select(.name | startswith("mcp-authorization-server") | not))]
  as $packages | .packages = $packages' \
  "$work_dir/good.json" >"$work_dir/bad.json"
reject "依存宣言の消失"
