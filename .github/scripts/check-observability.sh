#!/usr/bin/env bash
set -euo pipefail

status=0
source_root="${1:-crates}"
event_catalog="${2:-docs/observability.adoc}"
production_globs=(
  --glob '*.rs'
  --glob '!**/tests.rs'
  --glob '!**/tests/**'
)

if rg -n --pcre2 \
  '(^|[^:[:alnum:]_])(trace|debug|info|warn|error|span|trace_span|debug_span|info_span|warn_span|error_span)!\(' \
  "$source_root" \
  "${production_globs[@]}"
then
  echo "tracing macroはtracing::で修飾してください。" >&2
  status=1
fi

while IFS=: read -r source line _; do
  end=$((line + 2))
  if ! sed -n "${line},${end}p" "$source" |
    grep -Eq '\bevent[[:space:]]*=[[:space:]]*"[a-z][a-z0-9_.]*"'
  then
    echo "構造化ログの先頭3行に固定文字列のevent fieldがありません: $source:$line" >&2
    status=1
  fi
done < <(
  rg -n \
    'tracing::(trace|debug|info|warn|error)!\(' \
    "$source_root" \
    "${production_globs[@]}"
)

forbidden_field='(?:[A-Za-z0-9_]*(?:token|cookie|authorization_code|client_secret|note_id|header|origin)[A-Za-z0-9_]*|issuer|[A-Za-z0-9_]+_issuer|subject|[A-Za-z0-9_]+_subject|title|[A-Za-z0-9_]+_title|tags|[A-Za-z0-9_]+_tags|source|[A-Za-z0-9_]+_source|body|[A-Za-z0-9_]+_body|query|[A-Za-z0-9_]+_query|search|[A-Za-z0-9_]+_search|sec_fetch_site)'
tracing_macro='(?:trace|debug|info|warn|error|span|trace_span|debug_span|info_span|warn_span|error_span)'

if rg -n -U --pcre2 \
  "tracing::$tracing_macro!\\((?:(?!\\);)[\\s\\S]){0,1200}?\\b$forbidden_field\\s*(?:=|,)" \
  "$source_root" \
  "${production_globs[@]}"
then
  echo "構造化ログへ記録禁止fieldを追加しています。" >&2
  status=1
fi

if rg -n -U --pcre2 \
  "tracing::$tracing_macro!\\((?:(?!\\);)[\\s\\S]){0,1200}?\"(?:[^\"\\\\]|\\\\.)*\"\\s*,\\s*(?:%|\\?)?\\s*\\b$forbidden_field\\b" \
  "$source_root" \
  "${production_globs[@]}"
then
  echo "構造化ログ本文のformat引数へ記録禁止値を追加しています。" >&2
  status=1
fi

if rg -n -U --pcre2 \
  "tracing::$tracing_macro!\\((?:(?!\\);)[\\s\\S]){0,1200}?\\buri\\s*\\(" \
  "$source_root" \
  "${production_globs[@]}"
then
  echo "構造化ログへ未正規化のrequest URIを記録しています。" >&2
  status=1
fi

if [[ "$source_root" == "crates" ]]; then
  temporary_directory=$(mktemp -d)
  trap 'rm -rf "$temporary_directory"' EXIT
  implementation_events="$temporary_directory/implementation-events"
  documented_events="$temporary_directory/documented-events"
  documented_catalog="$temporary_directory/documented-catalog"
  event_pattern='(?:http|mcp|oidc|service|maintenance|command)(?:\.[a-z][a-z0-9_]*)+'
  documented_event_pattern="\`($event_pattern)\`"

  if [[ $(grep -c '^// observability-event-catalog:start$' "$event_catalog") -ne 1 ||
    $(grep -c '^// observability-event-catalog:end$' "$event_catalog") -ne 1 ]]
  then
    echo "ログevent一覧の開始・終了markerが一組ではありません。" >&2
    status=1
  fi
  sed -n \
    '/^\/\/ observability-event-catalog:start$/,/^\/\/ observability-event-catalog:end$/p' \
    "$event_catalog" >"$documented_catalog"

  # 共有Authorization Serverは外部crateだが、同じプロセスへ組み込むため運用者は同じjournalで
  # そのeventを見る。実装がMarginalisの外にあってもevent一覧との一致を確認する。
  embedded_sources=()
  while IFS= read -r manifest; do
    [[ -n "$manifest" ]] || continue
    embedded_source="$(dirname "$manifest")/src"
    if [[ ! -d "$embedded_source" ]]; then
      echo "組み込むcrateのソースが見つかりません: $embedded_source" >&2
      status=1
      continue
    fi
    embedded_sources+=("$embedded_source")
  done < <(
    cargo metadata --locked --format-version 1 |
      jq -r '.packages[]
        | select(.name == "mcp-authorization-server"
          or .name == "mcp-authorization-server-cimd")
        | .manifest_path'
  )
  if [[ "${#embedded_sources[@]}" -ne 2 ]]; then
    echo "組み込む共有Authorization Server crateが2件見つかりません。" >&2
    status=1
  fi

  rg --no-filename -o \
    "event[[:space:]]*=[[:space:]]*\"($event_pattern)\"" \
    -r '$1' \
    "$source_root" \
    ${embedded_sources[@]+"${embedded_sources[@]}"} \
    "${production_globs[@]}" |
    sort -u >"$implementation_events"
  rg --no-filename -o \
    "$documented_event_pattern" \
    -r '$1' \
    "$documented_catalog" |
    sort -u >"$documented_events"

  if ! diff -u "$documented_events" "$implementation_events"; then
    echo "production実装とログevent一覧が一致しません。" >&2
    echo "実装とdocs/observability.adocを同じ変更で更新してください。" >&2
    status=1
  fi
fi

exit "$status"
