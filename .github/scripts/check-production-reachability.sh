#!/usr/bin/env bash
set -euo pipefail

# marginalis-serviceの本番依存グラフを推移的にたどり、撤去済みのv0.2構成要素と経路が
# 復活していないことを確かめます。到達可能なworkspace crateの一覧が設計どおりで
# あること、撤去済みのroute名や設定名がソースに残っていないことを検査します。

temporary_directory=$(mktemp -d)
trap 'rm -rf "$temporary_directory"' EXIT

metadata="$temporary_directory/metadata.json"
metadata_input="${1:-}"
project_root="${2:-.}"

if [[ -n "$metadata_input" ]]; then
  cp "$metadata_input" "$metadata"
else
  cargo metadata --locked --format-version 1 >"$metadata"
fi

cd "$project_root"

actual=$(
  jq -r '
    . as $metadata
    | ($metadata.packages[]
        | select(.name == "marginalis-service")
        | .id) as $root
    | ($metadata.resolve.nodes
        | map({key: .id, value: [.deps[].pkg]})
        | from_entries) as $graph
    | def closure($pending; $seen):
        if ($pending | length) == 0 then $seen
        else
          [$pending[]
            | select(. as $id | ($seen | index($id) | not))] as $new
          | closure([$new[] as $id | $graph[$id][]?]; $seen + $new)
        end;
      closure([$root]; [])[] as $id
    | select($metadata.workspace_members | index($id))
    | $metadata.packages[]
    | select(.id == $id)
    | .name
  ' "$metadata" |
    sort -u
)
expected='marginalis-application
marginalis-archive
marginalis-asciidoc
marginalis-auth-oidc
marginalis-contract
marginalis-domain
marginalis-service
marginalis-sqlite
marginalis-web'
test "$actual" = "$expected" || {
  echo "marginalis-serviceの本番依存に想定外のworkspace crateがあります。" >&2
  diff -u <(printf '%s\n' "$expected") <(printf '%s\n' "$actual") >&2 || true
  exit 1
}

# tools/marginalis-documentation はCI専用で本番依存グラフに含まれないが、
# 撤去済みsymbolの残置検査は製品コードと同じ基準で行う。
if rg -n \
  '/api/v1|marginalis[_-](files|membership)|membershipTokenFile|root_password|MARGINALIS_ROOT' \
  Cargo.toml crates tools/marginalis-documentation --glob 'Cargo.toml' --glob '*.rs'; then
  echo "撤去済みのv0.2本番symbolまたはrouteが残っています。" >&2
  exit 1
fi

# 公開済みの記録は当時の名前のまま残す。ADRは検査の対象外とする。
if rg -n \
  'MARGINALIS_MCP_(AUTHORIZATION_ISSUER|UPSTREAM_ISSUER_CLAIM|UPSTREAM_SUBJECT_CLAIM|GROUPS_CLAIM|EXTERNAL_ISSUER)|externalAuthorization|marginalis-auth-oauth' \
  . \
  --glob '!docs/frozen/**' \
  --glob '!docs/developer-guide/adr/**' \
  --glob '!.github/scripts/check-production-reachability.sh' \
  --glob '!.github/scripts/test-production-reachability.sh' \
  --glob '!target/**' \
  --glob '!test-results/**' \
  --glob '!.git/**'; then
  echo "撤去済みの外部Authorization Server依存または設定が残っています。" >&2
  exit 1
fi

echo "本番依存グラフと撤去済みsymbolの不在を確認しました。"
