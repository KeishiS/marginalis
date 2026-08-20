#!/usr/bin/env bash
set -euo pipefail

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
work_dir=$(mktemp -d)
trap 'rm -rf "$work_dir"' EXIT

revision=1111111111111111111111111111111111111111
other_revision=2222222222222222222222222222222222222222
declaration="git+https://github.com/KeishiS/oidc-browser-login.git?rev=$revision"
package_source="$declaration#$revision"

jq -n \
  --arg declaration "$declaration" \
  --arg package_source "$package_source" '
{
  workspace_members: ["service", "sqlite"],
  packages: [
    {
      id: "service", name: "marginalis-service", version: "0.44.0",
      source: null, license: "MIT OR Apache-2.0",
      dependencies: [
        {name: "oidc-browser-login", kind: null, req: "=0.1.0", source: $declaration}
      ]
    },
    {
      id: "sqlite", name: "marginalis-sqlite", version: "0.44.0",
      source: null, license: "MIT OR Apache-2.0",
      dependencies: [
        {name: "oidc-browser-login", kind: "dev", req: "=0.1.0", source: $declaration},
        {name: "oidc-browser-login-testkit", kind: "dev", req: "=0.1.0", source: $declaration}
      ]
    },
    {
      id: "core", name: "oidc-browser-login", version: "0.1.0",
      source: $package_source, license: "MIT OR Apache-2.0", dependencies: []
    },
    {
      id: "testkit", name: "oidc-browser-login-testkit", version: "0.1.0",
      source: $package_source, license: "MIT OR Apache-2.0", dependencies: []
    }
  ],
  resolve: {
    nodes: [
      {id: "service", deps: [{pkg: "core", dep_kinds: [{kind: null}]}]},
      {id: "sqlite", deps: [
        {pkg: "core", dep_kinds: [{kind: "dev"}]},
        {pkg: "testkit", dep_kinds: [{kind: "dev"}]}
      ]},
      {id: "testkit", deps: [{pkg: "core", dep_kinds: [{kind: null}]}]},
      {id: "core", deps: []}
    ]
  }
}' >"$work_dir/good.json"

cat >"$work_dir/flake.nix" <<'EOF'
"oidc-browser-login-0.1.0" = "sha256-fixture";
"oidc-browser-login-testkit-0.1.0" = "sha256-fixture";
EOF

bash "$script_dir/check-shared-oidc-login.sh" \
  "$work_dir/good.json" "$work_dir/flake.nix" >/dev/null

reject() {
  local description=$1
  local flake_file="${2:-$work_dir/flake.nix}"
  if bash "$script_dir/check-shared-oidc-login.sh" \
    "$work_dir/bad.json" "$flake_file" >/dev/null 2>&1; then
    echo "受理してはいけない状態を受理しました: $description" >&2
    exit 1
  fi
}

# 版を範囲指定へ緩めた場合
jq '(.packages[] | select(.id == "service") | .dependencies[0] | .req) = "^0.1"' \
  "$work_dir/good.json" >"$work_dir/bad.json"
reject "版の範囲指定"

# 2 crateが別のcommitを指す場合
jq --arg source "git+https://github.com/KeishiS/oidc-browser-login.git?rev=$other_revision" \
  '(.packages[] | select(.id == "sqlite") | .dependencies[1] | .source) = $source' \
  "$work_dir/good.json" >"$work_dir/bad.json"
reject "revisionの不一致"

# revisionが短縮形の場合
jq --arg source "git+https://github.com/KeishiS/oidc-browser-login.git?rev=1111111" \
  '(.packages[] | select(.id == "service") | .dependencies[0] | .source) = $source' \
  "$work_dir/good.json" >"$work_dir/bad.json"
reject "短縮revision"

# 別リポジトリを指す場合
jq --arg source "git+https://example.test/oidc-browser-login.git?rev=$revision" \
  '(.packages[] | select(.id == "service") | .dependencies[0] | .source) = $source' \
  "$work_dir/good.json" >"$work_dir/bad.json"
reject "別リポジトリ"

# ローカルpathから解決された場合
jq '(.packages[] | select(.id == "core") | .source) = null' \
  "$work_dir/good.json" >"$work_dir/bad.json"
reject "ローカルpath版"

# 同じcrateが複数版存在する場合
jq --arg package_source "$package_source" '
  .packages += [{
    id: "core-old", name: "oidc-browser-login", version: "0.0.1",
    source: $package_source, license: "MIT OR Apache-2.0", dependencies: []
  }]' "$work_dir/good.json" >"$work_dir/bad.json"
reject "複数版の同居"

# testkitをproduction依存として宣言した場合
jq '(.packages[] | select(.id == "sqlite") | .dependencies[1] | .kind) = null' \
  "$work_dir/good.json" >"$work_dir/bad.json"
reject "production依存のtestkit"

# ライセンスが変わった場合
jq '(.packages[] | select(.id == "testkit") | .license) = "GPL-3.0"' \
  "$work_dir/good.json" >"$work_dir/bad.json"
reject "想定外のライセンス"

# testkitが中核へ依存しなくなった場合
jq '(.resolve.nodes[] | select(.id == "testkit") | .deps) = []' \
  "$work_dir/good.json" >"$work_dir/bad.json"
reject "testkitから中核への依存欠落"

# 本番依存から中核へ到達できなくなった場合
jq '(.resolve.nodes[] | select(.id == "service") | .deps[0] | .dep_kinds) = [{kind: "dev"}]' \
  "$work_dir/good.json" >"$work_dir/bad.json"
reject "本番依存からの到達不能"

# testkitが本番依存から到達できる場合
jq '(.resolve.nodes[] | select(.id == "service") | .deps) +=
  [{pkg: "testkit", dep_kinds: [{kind: null}]}]' \
  "$work_dir/good.json" >"$work_dir/bad.json"
reject "本番依存からのtestkit到達"

# 依存宣言そのものが消えた場合
jq '[.packages[] | .dependencies |= map(select(.name | startswith("oidc-browser-login") | not))]
  as $packages | .packages = $packages' \
  "$work_dir/good.json" >"$work_dir/bad.json"
reject "依存宣言の消失"

# Nixのcargoハッシュが欠けた場合
cat >"$work_dir/flake-missing.nix" <<'EOF'
"oidc-browser-login-0.1.0" = "sha256-fixture";
EOF
cp "$work_dir/good.json" "$work_dir/bad.json"
reject "Nixハッシュの欠落" "$work_dir/flake-missing.nix"
