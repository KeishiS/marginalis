#!/usr/bin/env bash
set -euo pipefail

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
work_dir=$(mktemp -d)
trap 'rm -rf "$work_dir"' EXIT

fixture="$work_dir/raw.log"
sanitized="$work_dir/artifact.log"
printf '%s\n' \
  'x-request-id: 01910000-0000-7000-8000-000000000001' \
  'location: https://client.example/callback?code=authorization-code-value&state=state-value' \
  'authorization: Bearer access-token-value' \
  'set-cookie: marginalis_session=session-value; Secure' \
  '{"access_token":"access-token-value","refresh_token":"refresh-token-value","csrf_token":"csrf-value","code_verifier":"pkce-value"}' \
  'grant_type=authorization_code&code=form-code-value&client_secret=form-secret-value' \
  >"$fixture"

"$script_dir/protocol-artifact.sh" sanitize "$fixture" "$sanitized"
grep -Fq 'x-request-id: 01910000-0000-7000-8000-000000000001' "$sanitized"
grep -Fq '[REDACTED]' "$sanitized"
! grep -Fq 'access-token-value' "$sanitized"
! grep -Fq 'refresh-token-value' "$sanitized"
! grep -Fq 'authorization-code-value' "$sanitized"
! grep -Fq 'csrf-value' "$sanitized"
! grep -Fq 'pkce-value' "$sanitized"
! grep -Fq 'form-code-value' "$sanitized"
! grep -Fq 'form-secret-value' "$sanitized"
"$script_dir/protocol-artifact.sh" check "$sanitized"

printf '%s\n' \
  'cookie: must-be-detected' \
  '{"cookies":[{"name":"marginalis_session","value":"must-be-detected"}]}' \
  >"$work_dir/leaked.log"
if "$script_dir/protocol-artifact.sh" check "$work_dir/leaked.log" >/dev/null 2>&1; then
  echo "漏洩検査がCookieを検出しませんでした。" >&2
  exit 1
fi
