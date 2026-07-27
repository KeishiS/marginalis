#!/usr/bin/env bash
set -euo pipefail

usage() {
  echo "使い方: $0 sanitize <入力> <出力> | check <ファイルまたはディレクトリ>" >&2
  exit 2
}

secret_pattern='([Aa]uthorization:[[:space:]]*(Bearer|Basic)[[:space:]]+(?!\[REDACTED\])[^[:space:]]+|([Ss]et-[Cc]ookie:|[Cc]ookie:)[[:space:]]*(?!\[REDACTED\])[^[:space:]]+|"(access_token|refresh_token|id_token|client_secret|authorization_code)"[[:space:]]*:[[:space:]]*"(?!\[REDACTED\])[^"]+"|[?&](code|access_token|refresh_token|client_secret)=(?!\[REDACTED\])[^&[:space:]]+)'

sanitize() {
  local input=$1
  local output=$2
  umask 077
  sed -E \
    -e 's#([Aa]uthorization:[[:space:]]*(Bearer|Basic))[[:space:]]+[^[:space:]]+#\1 [REDACTED]#g' \
    -e 's#([Ss]et-[Cc]ookie:|[Cc]ookie:).*#\1 [REDACTED]#g' \
    -e 's#("(access_token|refresh_token|id_token|client_secret|authorization_code)"[[:space:]]*:[[:space:]]*)"[^"]*"#\1"[REDACTED]"#g' \
    -e 's#([?&](code|access_token|refresh_token|client_secret)=)[^&[:space:]]+#\1[REDACTED]#g' \
    "$input" >"$output"
}

check() {
  local target=$1
  if rg --pcre2 -n "$secret_pattern" "$target"; then
    echo "失敗証跡に秘密情報を示す項目が残っています。" >&2
    exit 1
  fi
}

case "${1-}" in
  sanitize)
    test "$#" -eq 3 || usage
    sanitize "$2" "$3"
    ;;
  check)
    test "$#" -eq 2 || usage
    check "$2"
    ;;
  *)
    usage
    ;;
esac
