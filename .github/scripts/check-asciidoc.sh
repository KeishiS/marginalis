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

# lockfileとNode.jsの版が前回の導入時から変わらない限り、npm ciのネットワークアクセスを
# 省略する。最新情報を取得する脆弱性監査は、再現可能な文書検査と分けてsecurity-auditで行う。
textlint_root=tools/textlint
stamp_file="$textlint_root/node_modules/.marginalis-install-stamp"
stamp_value="$(node --version) $(sha256sum "$textlint_root/package-lock.json" | cut -d' ' -f1)"
if [[ ! -f "$stamp_file" || "$(cat "$stamp_file")" != "$stamp_value" ]]; then
  npm ci --ignore-scripts --prefix "$textlint_root" >/dev/null
  printf '%s' "$stamp_value" >"$stamp_file"
fi
npm test --silent --prefix "$textlint_root"
npm run --silent --prefix "$textlint_root" lint -- "${documents[@]}"

echo "AsciiDoc文書を検査しました: ${document_count}件"
