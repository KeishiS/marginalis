#!/usr/bin/env bash
set -euo pipefail

release_guide="${1:-docs/developer-guide/release.adoc}"

fail() {
  echo "$1" >&2
  exit 1
}

mapfile -t release_commands < <(grep -E '^gh release create ' "$release_guide" || true)
if [[ "${#release_commands[@]}" -ne 1 ]]; then
  fail "リリース手順にはgh release createコマンドを一つだけ記載してください。"
fi

release_command=" ${release_commands[0]} "
if [[ "$release_command" != *" --draft "* ]]; then
  fail "GitHub Releaseはassetを揃えるまで下書きとして作成してください。"
fi
for asset in docs/openapi.json docs/mcp-tools.json; do
  if [[ "$release_command" != *" $asset "* ]]; then
    fail "GitHub Releaseへ添付する公開契約がリリース手順にありません: $asset"
  fi
done

if ! grep -Eq '^gh release view .* --json isDraft,assets ' "$release_guide"; then
  fail "GitHub Releaseの下書き状態とassetを公開前に確認してください。"
fi
if ! grep -Eq '^gh release edit .* --draft=false$' "$release_guide"; then
  fail "確認済みのGitHub Releaseを明示的に公開する手順がありません。"
fi

echo "GitHub Releaseの下書き作成、公開契約の添付、明示的な公開手順を確認しました。"
