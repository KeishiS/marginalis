#!/usr/bin/env bash
set -euo pipefail

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
work_dir=$(mktemp -d)
trap 'rm -rf "$work_dir"' EXIT

pinned_revision=1111111111111111111111111111111111111111
new_revision=2222222222222222222222222222222222222222
tag_object_sha=3333333333333333333333333333333333333333
source="git+https://github.com/KeishiS/mcp-authorization-server.git?rev=$pinned_revision"

jq -n --arg source "$source" '{
  workspace_members: ["service"],
  packages: [{
    id: "service", name: "marginalis-service",
    dependencies: [
      {name: "mcp-authorization-server", req: "=0.1.0", source: $source},
      {name: "mcp-authorization-server-cimd", req: "=0.1.0", source: $source}
    ]
  }]
}' >"$work_dir/metadata.json"

release() {
  local tag=$1
  local draft=$2
  local prerelease=$3
  local published_at=$4
  jq -n --arg tag "$tag" --argjson draft "$draft" --argjson prerelease "$prerelease" \
    --arg published_at "$published_at" '{
      tag_name: $tag,
      draft: $draft,
      prerelease: $prerelease,
      published_at: $published_at,
      html_url: ("https://github.com/KeishiS/mcp-authorization-server/releases/tag/" + $tag)
    }'
}

release v0.1.0 false false 2026-08-06T00:00:00Z | jq -s '.' >"$work_dir/releases-current.json"
jq -n --arg sha "$tag_object_sha" '{object: {type: "tag", sha: $sha}}' >"$work_dir/tag-reference.json"
jq -n --arg sha "$pinned_revision" '{object: {type: "commit", sha: $sha}}' >"$work_dir/tag-current.json"
jq -n --arg sha "$new_revision" '{object: {type: "commit", sha: $sha}}' >"$work_dir/tag-new.json"

check_with() {
  local releases=$1
  local tag_object=$2
  bash "$script_dir/check-shared-authorization-server-release.sh" \
    --metadata "$work_dir/metadata.json" \
    --releases "$releases" \
    --tag-reference "$work_dir/tag-reference.json" \
    --tag-object "$tag_object"
}

# 固定内容と最新の通常Releaseが一致する場合
check_with "$work_dir/releases-current.json" "$work_dir/tag-current.json" >/dev/null

# 新しい通常Releaseがある場合は、通信失敗とは異なる終了状態10を返すこと
release v0.2.0 false false 2026-08-07T00:00:00Z | jq -s '.' >"$work_dir/releases-new.json"
set +e
check_with "$work_dir/releases-new.json" "$work_dir/tag-new.json" >/dev/null 2>&1
status=$?
set -e
[[ "$status" -eq 10 ]] || {
  echo "更新ありを終了状態10で報告しませんでした: $status" >&2
  exit 1
}

# より新しいdraftとprereleaseを除外し、通常Releaseだけを比較すること
{
  release v0.1.0 false false 2026-08-06T00:00:00Z
  release v0.2.0 false true 2026-08-07T00:00:00Z
  release v0.3.0 true false 2026-08-08T00:00:00Z
} | jq -s '.' >"$work_dir/releases-filtered.json"
check_with "$work_dir/releases-filtered.json" "$work_dir/tag-current.json" >/dev/null

# gh api --paginate --slurpが返すpageごとの配列も受理すること
jq -n --slurpfile releases "$work_dir/releases-current.json" '$releases' >"$work_dir/releases-pages.json"
check_with "$work_dir/releases-pages.json" "$work_dir/tag-current.json" >/dev/null

# 同じ版のタグが別commitを指す場合も差異として報告すること
set +e
check_with "$work_dir/releases-current.json" "$work_dir/tag-new.json" >/dev/null 2>&1
status=$?
set -e
[[ "$status" -eq 10 ]] || {
  echo "タグの対象commitの差異を終了状態10で報告しませんでした: $status" >&2
  exit 1
}

# 軽量タグはannotated tagとして受理しないこと
jq -n --arg sha "$pinned_revision" '{object: {type: "commit", sha: $sha}}' >"$work_dir/lightweight-reference.json"
set +e
bash "$script_dir/check-shared-authorization-server-release.sh" \
  --metadata "$work_dir/metadata.json" \
  --releases "$work_dir/releases-current.json" \
  --tag-reference "$work_dir/lightweight-reference.json" \
  --tag-object "$work_dir/tag-current.json" >/dev/null 2>&1
status=$?
set -e
[[ "$status" -eq 2 ]] || {
  echo "軽量タグを検査失敗として報告しませんでした: $status" >&2
  exit 1
}

# GitHub APIの通信失敗は更新ありとは異なる終了状態2を返すこと
mkdir "$work_dir/bin"
printf '%s\n' '#!/usr/bin/env bash' 'exit 1' >"$work_dir/bin/gh"
chmod +x "$work_dir/bin/gh"
set +e
PATH="$work_dir/bin:$PATH" bash "$script_dir/check-shared-authorization-server-release.sh" \
  --metadata "$work_dir/metadata.json" >/dev/null 2>&1
status=$?
set -e
[[ "$status" -eq 2 ]] || {
  echo "通信失敗を終了状態2で報告しませんでした: $status" >&2
  exit 1
}

echo "共有Authorization Serverの公開版検査を確認しました。"
