#!/usr/bin/env bash
set -euo pipefail

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
work_dir=$(mktemp -d)
trap 'rm -rf "$work_dir"' EXIT

write_workflow() {
  cat >"$work_dir/release-gate.yml" <<'EOF'
          if [ "$state" = "absent" ]; then
            gh release create "$RELEASE_TAG" --verify-tag --draft \
              docs/openapi.json docs/mcp-tools.json \
              --title "$RELEASE_TAG" \
              --notes "Release Notesを記載してから公開してください。"
          fi
          test "$(gh release view "$RELEASE_TAG" --json isDraft,assets \
            --jq '(.isDraft == true) and (([.assets[].name] | sort) == ["mcp-tools.json", "openapi.json"])')" = true
EOF
}

write_guide() {
  cat >"$work_dir/release.adoc" <<'EOF'
release_tag=v<MAJOR>.<MINOR>.<PATCH>
gh release edit "$release_tag" --notes-file <Release Notesのファイル>
test "$(gh release view "$release_tag" --json isDraft,assets \
  --jq '(.isDraft == true) and (([.assets[].name] | sort) == ["mcp-tools.json", "openapi.json"])')" = true
gh release edit "$release_tag" --draft=false
EOF
}

rebuild() {
  write_guide
  write_workflow
}

rebuild
bash "$script_dir/check-release-instructions.sh" \
  "$work_dir/release.adoc" "$work_dir/release-gate.yml" >/dev/null

reject() {
  local description=$1
  if bash "$script_dir/check-release-instructions.sh" \
    "$work_dir/release.adoc" "$work_dir/release-gate.yml" >/dev/null 2>&1; then
    echo "受理してはいけないリリース手順を受理しました: $description" >&2
    exit 1
  fi
}

rebuild
sed -i 's/ --verify-tag//' "$work_dir/release-gate.yml"
reject "既存タグ確認の欠落"

rebuild
sed -i 's# docs/mcp-tools.json##' "$work_dir/release-gate.yml"
reject "MCP tool契約assetの欠落"

rebuild
sed -i 's/gh release view "$RELEASE_TAG"/gh release view v9.9.9/' "$work_dir/release-gate.yml"
reject "workflowの確認対象タグの不一致"

rebuild
sed -i 's/(.isDraft == true) and ((\[.assets\[\].name\] | sort) == \["mcp-tools.json", "openapi.json"\])/.isDraft/' \
  "$work_dir/release-gate.yml"
reject "workflowの下書き状態とasset集合の不完全な確認"

rebuild
sed -i '/--notes-file/d' "$work_dir/release.adoc"
reject "Release Notes記載手順の欠落"

rebuild
sed -i 's/gh release view "$release_tag"/gh release view v9.9.9/' "$work_dir/release.adoc"
reject "確認対象タグの不一致"

rebuild
sed -i 's/(.isDraft == true) and ((\[.assets\[\].name\] | sort) == \["mcp-tools.json", "openapi.json"\])/.isDraft/' \
  "$work_dir/release.adoc"
reject "下書き状態とasset集合の不完全な確認"

rebuild
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
