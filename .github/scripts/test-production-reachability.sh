#!/usr/bin/env bash
set -euo pipefail

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
work_dir=$(mktemp -d)
trap 'rm -rf "$work_dir"' EXIT

# 設計どおりの本番依存グラフと、撤去済みsymbolのないソース木を組み立てます。
build_tree() {
  local root=$1
  mkdir -p "$root/crates/marginalis-service/src" \
    "$root/tools/marginalis-documentation/src" \
    "$root/docs/developer-guide/adr"
  echo '[workspace]' >"$root/Cargo.toml"
  echo 'fn main() {}' >"$root/crates/marginalis-service/src/main.rs"
  echo 'fn main() {}' >"$root/tools/marginalis-documentation/src/main.rs"
  jq -n '
    ["marginalis-application", "marginalis-archive", "marginalis-asciidoc",
     "marginalis-auth-oidc", "marginalis-contract", "marginalis-domain",
     "marginalis-service", "marginalis-sqlite", "marginalis-web",
     "marginalis-webhook-http",
     "marginalis-documentation"] as $names
    | {
        workspace_members: $names,
        packages: [$names[] | {id: ., name: ., version: "0.36.0"}],
        resolve: {
          nodes: (
            [{id: "marginalis-service",
              deps: [($names[] | select(. != "marginalis-service"
                and . != "marginalis-documentation")) | {pkg: .}]}]
            + [($names[] | select(. != "marginalis-service"))
                | {id: ., deps: []}]
          )
        }
      }
  ' >"$root/metadata.json"
}

build_tree "$work_dir/good"
bash "$script_dir/check-production-reachability.sh" \
  "$work_dir/good/metadata.json" "$work_dir/good" >/dev/null

reject() {
  local description=$1
  if bash "$script_dir/check-production-reachability.sh" \
    "$work_dir/bad/metadata.json" "$work_dir/bad" >/dev/null 2>&1; then
    echo "受理してはいけない状態を受理しました: $description" >&2
    exit 1
  fi
}

rebuild() {
  rm -rf "$work_dir/bad"
  build_tree "$work_dir/bad"
}

# CI専用crateが本番依存グラフへ到達可能になった場合
rebuild
jq '(.resolve.nodes[] | select(.id == "marginalis-service") | .deps)
      += [{pkg: "marginalis-documentation"}]' \
  "$work_dir/bad/metadata.json" >"$work_dir/bad/metadata.new"
mv "$work_dir/bad/metadata.new" "$work_dir/bad/metadata.json"
reject "CI専用crateの本番到達"

# 設計上必要なcrateへ到達できなくなった場合
rebuild
jq '(.resolve.nodes[] | select(.id == "marginalis-service") | .deps)
      |= map(select(.pkg != "marginalis-archive"))' \
  "$work_dir/bad/metadata.json" >"$work_dir/bad/metadata.new"
mv "$work_dir/bad/metadata.new" "$work_dir/bad/metadata.json"
reject "設計上のcrateへの到達不能"

# 撤去済みのrouteが製品コードへ復活した場合
rebuild
echo 'const ROUTE: &str = "/api/v1";' >>"$work_dir/bad/crates/marginalis-service/src/main.rs"
reject "撤去済みrouteの復活"

# 撤去済みの設定名がCI専用crateへ復活した場合
rebuild
echo 'const KEY: &str = "membershipTokenFile";' \
  >>"$work_dir/bad/tools/marginalis-documentation/src/main.rs"
reject "CI専用crateでの撤去済み設定名"

# 撤去済みの外部Authorization Server設定が文書へ復活した場合
rebuild
echo 'externalAuthorization' >"$work_dir/bad/docs/retired.adoc"
reject "撤去済み外部AS設定の復活"

# ADRに残る公開済みの記録は許容する場合
rebuild
echo 'marginalis-auth-oauth' >"$work_dir/bad/docs/developer-guide/adr/0001-example.adoc"
bash "$script_dir/check-production-reachability.sh" \
  "$work_dir/bad/metadata.json" "$work_dir/bad" >/dev/null

echo "本番到達性検査の自己テストに成功しました。"
