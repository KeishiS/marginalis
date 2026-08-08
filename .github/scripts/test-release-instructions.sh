#!/usr/bin/env bash
set -euo pipefail

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
work_dir=$(mktemp -d)
trap 'rm -rf "$work_dir"' EXIT

write_guide() {
  cat >"$work_dir/release.adoc" <<'EOF'
release_tag=v<MAJOR>.<MINOR>.<PATCH>
release_assets=$(mktemp -d)
trap 'rm -rf "$release_assets"' EXIT
git show "$release_tag:docs/openapi.json" >"$release_assets/openapi.json"
git show "$release_tag:docs/mcp-tools.json" >"$release_assets/mcp-tools.json"
gh release create "$release_tag" --verify-tag --draft \
  "$release_assets/openapi.json" "$release_assets/mcp-tools.json" \
  --title "$release_tag" --notes-file <Release Notesのファイル>
test "$(gh release view "$release_tag" --json isDraft,assets \
  --jq '(.isDraft == true) and (([.assets[].name] | sort) == ["mcp-tools.json", "openapi.json"])')" = true
gh release edit "$release_tag" --draft=false
EOF
}

write_guide
bash "$script_dir/check-release-instructions.sh" "$work_dir/release.adoc" >/dev/null

reject() {
  local description=$1
  if bash "$script_dir/check-release-instructions.sh" "$work_dir/release.adoc" \
    >/dev/null 2>&1; then
    echo "受理してはいけないリリース手順を受理しました: $description" >&2
    exit 1
  fi
}

write_guide
sed -i 's/ --verify-tag//' "$work_dir/release.adoc"
reject "既存タグ確認の欠落"

write_guide
sed -i 's#git show "$release_tag:docs/openapi.json"#cp docs/openapi.json#' "$work_dir/release.adoc"
reject "作業treeからのasset取得"

write_guide
sed -i \
  -e '/git show "$release_tag:docs\/mcp-tools.json"/d' \
  -e 's/ "$release_assets\/mcp-tools.json"//' \
  "$work_dir/release.adoc"
reject "MCP tool契約assetの欠落"

write_guide
sed -i 's/gh release view "$release_tag"/gh release view v9.9.9/' "$work_dir/release.adoc"
reject "確認対象タグの不一致"

write_guide
sed -i 's/(.isDraft == true) and ((\[.assets\[\].name\] | sort) == \["mcp-tools.json", "openapi.json"\])/.isDraft/' \
  "$work_dir/release.adoc"
reject "下書き状態とasset集合の不完全な確認"

write_guide
awk '
  /^test "\$\(gh release view / {
    print "gh release edit \"$release_tag\" --draft=false"
    print
    next
  }
  /^gh release edit .*--draft=false$/ { next }
  { print }
' "$work_dir/release.adoc" >"$work_dir/reordered.adoc"
mv "$work_dir/reordered.adoc" "$work_dir/release.adoc"
reject "確認前の公開"

echo "リリース手順検査の自己テストに成功しました。"
