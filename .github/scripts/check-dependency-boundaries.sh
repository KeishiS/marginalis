#!/usr/bin/env bash
set -euo pipefail

temporary_directory=$(mktemp -d)
trap 'rm -rf "$temporary_directory"' EXIT

metadata="$temporary_directory/metadata.json"
actual="$temporary_directory/actual"
external_dependencies="$temporary_directory/external-dependencies"
metadata_input="${1:-}"
expected_input="${2:-}"

if [[ -n "$metadata_input" ]]; then
  cp "$metadata_input" "$metadata"
else
  cargo metadata --locked --format-version 1 >"$metadata"
fi

jq -r '
  . as $metadata
  | $metadata.packages[]
  | select(.id as $id | $metadata.workspace_members | index($id))
  | .name as $package
  | [.dependencies[]
      | select(.kind == null and .path != null)
      | .name]
  | sort
  | if length == 0 then
      "\($package):"
    else
      "\($package): \(join(", "))"
    end
' "$metadata" |
  sort >"$actual"

if [[ -n "$expected_input" ]]; then
  expected="$expected_input"
else
  expected="$temporary_directory/expected"
  cat >"$expected" <<'EOF'
marginalis-application: marginalis-domain, mcp-authorization-server
marginalis-archive: marginalis-application, marginalis-domain
marginalis-asciidoc: marginalis-application, marginalis-domain
marginalis-auth-oidc: marginalis-application, marginalis-domain
marginalis-contract: marginalis-domain
marginalis-domain:
marginalis-service: marginalis-application, marginalis-archive, marginalis-asciidoc, marginalis-auth-oidc, marginalis-domain, marginalis-sqlite, marginalis-web
marginalis-sqlite: marginalis-application, marginalis-domain
marginalis-web: marginalis-application, marginalis-contract, marginalis-domain
mcp-authorization-server:
EOF
fi

if ! diff -u "$expected" "$actual"; then
  echo "workspace crateのproduction依存が設計境界と一致しません。" >&2
  echo "domainは他のcrateへ依存せず、contractとapplicationはdomainだけへ依存します。" >&2
  echo "adapter同士は依存せず、具象adapterの組み立てはserviceだけで行います。" >&2
  exit 1
fi

jq -r '
  . as $metadata
  | $metadata.packages[]
  | select(.id as $id | $metadata.workspace_members | index($id))
  | .name as $package
  | .dependencies[]
  | select(.kind == null and .path == null)
  | [$package, .name]
  | @tsv
' "$metadata" >"$external_dependencies"

while IFS=$'\t' read -r package dependency; do
  case "$package:$dependency" in
    marginalis-domain:axum | marginalis-domain:sqlx | marginalis-domain:reqwest | \
      marginalis-domain:adocweave | marginalis-domain:openidconnect | \
      marginalis-domain:oauth2 | marginalis-domain:tower | marginalis-domain:tower-http | \
      marginalis-application:axum | marginalis-application:sqlx | \
      marginalis-application:reqwest | marginalis-application:adocweave | \
      marginalis-application:openidconnect | marginalis-application:oauth2 | \
      marginalis-application:tower | marginalis-application:tower-http | \
      marginalis-contract:axum | marginalis-contract:sqlx | marginalis-contract:reqwest | \
      marginalis-contract:adocweave | marginalis-contract:openidconnect | \
      marginalis-contract:oauth2 | marginalis-contract:tower | marginalis-contract:tower-http | \
      mcp-authorization-server:axum | mcp-authorization-server:sqlx | \
      mcp-authorization-server:reqwest | mcp-authorization-server:adocweave | \
      mcp-authorization-server:openidconnect | mcp-authorization-server:oauth2 | \
      mcp-authorization-server:tower | mcp-authorization-server:tower-http | \
      marginalis-web:sqlx | marginalis-web:adocweave | marginalis-web:openidconnect | \
      marginalis-web:oauth2 | marginalis-web:reqwest)
      echo "内側の層またはHTTP transportへ具象adapter依存が混入しています: $package -> $dependency" >&2
      exit 1
      ;;
  esac
done <"$external_dependencies"

if [[ -z "$metadata_input" ]] && cargo tree -p marginalis-web -e normal |
  grep -E 'marginalis-(sqlite|asciidoc|auth-oidc)|(^| )sqlx |(^| )adocweave |(^| )openidconnect |(^| )reqwest ' >/dev/null; then
  echo "HTTP transportのproduction依存へ具象adapterが混入しています。" >&2
  exit 1
fi
