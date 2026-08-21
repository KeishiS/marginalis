#!/usr/bin/env bash
set -euo pipefail

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
work_dir=$(mktemp -d)
trap 'rm -rf "$work_dir"' EXIT

build_tree() {
  local root=$1
  mkdir -p "$root/docs" "$root/release"
  cat >"$root/Cargo.toml" <<'EOF'
[workspace]
members = ["crates/marginalis-domain"]

[workspace.package]
version = "1.2.3"
edition = "2024"
rust-version = "1.97.1"
EOF
  jq -n '{
    schemaVersion: 1,
    product: "marginalis",
    packageVersion: "1.2.3",
    rustVersion: "1.97.1",
    nodeVersion: "22.23.1",
    releaseNotes: "release/notes.md",
    assets: [
      {name: "mcp-tools.json", source: "docs/mcp-tools.json"},
      {name: "openapi.json", source: "docs/openapi.json"}
    ]
  }' >"$root/release-manifest.json"
  echo '{}' >"$root/docs/mcp-tools.json"
  echo '{}' >"$root/docs/openapi.json"
  cat >"$root/release/notes.md" <<'EOF'
# Marginalis v1.2.3

## 主な変更

ノートの版履歴を追加しました。

## 対応環境

x86_64とaarch64のLinuxで動作します。

## 公開契約と破壊的変更

破壊的変更はありません。

## v1.2.3への移行

移行は不要です。

## 更新とロールバック

直前のNixOS generationへ戻せます。

## 既知の制約

添付できるのは画像だけです。

## 配布物の検証

assetへattestationを付与しています。
EOF
}

build_tree "$work_dir/good"
bash "$script_dir/check-release-manifest.sh" "$work_dir/good" >/dev/null

# 開発環境のNode.jsとの照合は明示的に要求したときだけ行います。
build_tree "$work_dir/runtime"
jq '.nodeVersion = "0.0.1"' "$work_dir/runtime/release-manifest.json" >"$work_dir/runtime/new.json"
mv "$work_dir/runtime/new.json" "$work_dir/runtime/release-manifest.json"
bash "$script_dir/check-release-manifest.sh" "$work_dir/runtime" >/dev/null
if bash "$script_dir/check-release-manifest.sh" --with-runtime-toolchain "$work_dir/runtime" \
  >/dev/null 2>&1; then
  echo "実行環境と異なるnodeVersionを受理しました。" >&2
  exit 1
fi

reject() {
  local description=$1
  if bash "$script_dir/check-release-manifest.sh" "$work_dir/bad" >/dev/null 2>&1; then
    echo "受理してはいけない状態を受理しました: $description" >&2
    exit 1
  fi
}

rebuild() {
  rm -rf "$work_dir/bad"
  build_tree "$work_dir/bad"
}

edit_manifest() {
  jq "$1" "$work_dir/bad/release-manifest.json" >"$work_dir/bad/manifest.new"
  mv "$work_dir/bad/manifest.new" "$work_dir/bad/release-manifest.json"
}

# manifestの版がworkspaceの版から外れた場合
rebuild
edit_manifest '.packageVersion = "1.2.4"'
reject "workspaceの版との不一致"

# 版を上げてもRelease Notesを更新し忘れた場合
rebuild
sed -i 's/^version = "1.2.3"$/version = "1.3.0"/' "$work_dir/bad/Cargo.toml"
edit_manifest '.packageVersion = "1.3.0"'
reject "Release Notesの版の取り残し"

# toolchainの宣言がCargo.tomlと食い違う場合
rebuild
edit_manifest '.rustVersion = "1.90.0"'
reject "rust-versionとの不一致"

# 宣言したassetが実在しない場合
rebuild
rm "$work_dir/bad/docs/openapi.json"
reject "assetの欠落"

# 同じassetを二重に宣言した場合
rebuild
edit_manifest '.assets += [{name: "openapi.json", source: "docs/openapi.json"}]'
reject "assetの重複宣言"

# 公開するassetを一つも宣言しない場合
rebuild
edit_manifest '.assets = []'
reject "assetの未宣言"

# Release Notesの位置が実在しない場合
rebuild
rm "$work_dir/bad/release/notes.md"
reject "Release Notesの欠落"

# 必須の見出しが欠けた場合
rebuild
sed -i '/^## 既知の制約$/,+2d' "$work_dir/bad/release/notes.md"
reject "必須見出しの欠落"

# 必須の見出しの順序が入れ替わった場合
rebuild
cat >"$work_dir/bad/release/notes.md" <<'EOF'
# Marginalis v1.2.3

## 対応環境

x86_64とaarch64のLinuxで動作します。

## 主な変更

ノートの版履歴を追加しました。

## 公開契約と破壊的変更

破壊的変更はありません。

## v1.2.3への移行

移行は不要です。

## 更新とロールバック

直前のNixOS generationへ戻せます。

## 既知の制約

添付できるのは画像だけです。

## 配布物の検証

assetへattestationを付与しています。
EOF
reject "必須見出しの順序違反"

# 見出しだけがあって本文がない場合
rebuild
sed -i 's/^添付できるのは画像だけです。$//' "$work_dir/bad/release/notes.md"
reject "本文のない節"

# 未記入の目印が残った場合
rebuild
sed -i 's/^移行は不要です。$/TODO/' "$work_dir/bad/release/notes.md"
reject "未記入の目印"

# schemaVersionが想定と異なる場合
rebuild
edit_manifest '.schemaVersion = 2'
reject "未対応のschemaVersion"

echo "release manifest検査の自己テストに成功しました。"
