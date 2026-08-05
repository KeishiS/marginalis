#!/usr/bin/env bash
set -euo pipefail

adocweave config show --config .adocweave.toml >/dev/null

document_count=0
status=0
documents=()
while IFS= read -r -d '' source; do
  [[ -f "$source" ]] || continue
  document_count=$((document_count + 1))
  documents+=("$source")
  if ! adocweave check \
    --fail-on warning \
    --local-targets \
    --project-root . \
    "$source"; then
    status=1
  fi
done < <(
  git ls-files --cached --others --exclude-standard -z -- '*.adoc' |
    sort -z
)

if [[ "$document_count" -eq 0 ]]; then
  echo "検査対象のAsciiDoc文書がありません。" >&2
  exit 1
fi
if [[ "$status" -ne 0 ]]; then
  echo "AsciiDoc文書の検査に失敗しました。" >&2
  exit "$status"
fi

cargo run --quiet --locked -p marginalis-documentation -- \
  check-xrefs --project-root . "${documents[@]}"
npm ci --ignore-scripts --prefix tools/textlint >/dev/null
npm audit --audit-level=high --prefix tools/textlint >/dev/null
npm test --silent --prefix tools/textlint
npm run --silent --prefix tools/textlint lint -- "${documents[@]}"

echo "AsciiDoc文書を検査しました: ${document_count}件"
