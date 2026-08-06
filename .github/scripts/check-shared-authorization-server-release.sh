#!/usr/bin/env bash
set -euo pipefail

# 上流の最新の通常Releaseを、Marginalisが固定する版とcommit SHAの両方に照合します。
# 終了状態は、0が一致、10が更新あり、2が通信または上流データの検査失敗です。

repository='KeishiS/mcp-authorization-server'
repository_url='https://github.com/KeishiS/mcp-authorization-server.git'
core='mcp-authorization-server'
cimd='mcp-authorization-server-cimd'

metadata_input=''
releases_input=''
tag_reference_input=''
tag_object_input=''

while [[ $# -gt 0 ]]; do
  case "$1" in
    --metadata)
      metadata_input=${2:-}
      shift 2
      ;;
    --releases)
      releases_input=${2:-}
      shift 2
      ;;
    --tag-reference)
      tag_reference_input=${2:-}
      shift 2
      ;;
    --tag-object)
      tag_object_input=${2:-}
      shift 2
      ;;
    *)
      echo "未対応の引数です: $1" >&2
      exit 2
      ;;
  esac
done

temporary_directory=$(mktemp -d)
trap 'rm -rf "$temporary_directory"' EXIT

fail() {
  echo "$1" >&2
  exit 2
}

copy_or_fetch() {
  local input=$1
  local endpoint=$2
  local output=$3
  if [[ -n "$input" ]]; then
    cp "$input" "$output" || fail "検査用JSONを読み込めませんでした: $input"
    return
  fi
  if ! gh api "$endpoint" >"$output"; then
    fail "GitHub APIから上流情報を取得できませんでした: $endpoint"
  fi
}

copy_or_fetch_releases() {
  local input=$1
  local endpoint=$2
  local output=$3
  if [[ -n "$input" ]]; then
    cp "$input" "$output" || fail "検査用JSONを読み込めませんでした: $input"
    return
  fi
  if ! gh api --paginate --slurp "$endpoint" >"$output"; then
    fail "GitHub APIから上流情報を取得できませんでした: $endpoint"
  fi
}

metadata="$temporary_directory/metadata.json"
if [[ -n "$metadata_input" ]]; then
  cp "$metadata_input" "$metadata" || fail "Cargo metadataを読み込めませんでした: $metadata_input"
elif ! cargo metadata --locked --no-deps --format-version 1 >"$metadata"; then
  fail "Marginalisが固定するAuthorization Serverの版とrevisionを取得できませんでした。"
fi

declarations="$temporary_directory/declarations.tsv"
if ! jq -er --arg core "$core" --arg cimd "$cimd" '
  . as $metadata
  | $metadata.packages[]
  | select(.id as $id | $metadata.workspace_members | index($id))
  | .dependencies[]
  | select(.name == $core or .name == $cimd)
  | [.name, .req, (.source // "")]
  | @tsv
' "$metadata" | sort -u >"$declarations"; then
  fail "Cargo metadataからAuthorization Serverの依存宣言を読み取れませんでした。"
fi

[[ -s "$declarations" ]] || fail "Authorization Serverの依存宣言が見つかりませんでした。"

versions=$(awk -F'\t' '{ print $2 }' "$declarations" | sort -u)
sources=$(awk -F'\t' '{ print $3 }' "$declarations" | sort -u)
[[ $(printf '%s\n' "$versions" | wc -l) -eq 1 ]] || fail "2 crateの固定版が一致しません。"
[[ $(printf '%s\n' "$sources" | wc -l) -eq 1 ]] || fail "2 crateの固定revisionが一致しません。"

pinned_requirement=$versions
pinned_source=$sources
[[ "$pinned_requirement" =~ ^=([0-9]+\.[0-9]+\.[0-9]+)$ ]] || fail "固定版が完全な通常版ではありません: $pinned_requirement"
pinned_version=${BASH_REMATCH[1]}
if [[ "$pinned_source" =~ ^git\+${repository_url//./\.}\?rev=([0-9a-f]{40})$ ]]; then
  pinned_revision=${BASH_REMATCH[1]}
else
  fail "固定revisionが上流リポジトリの40桁の完全SHAではありません。"
fi

releases="$temporary_directory/releases.json"
copy_or_fetch_releases "$releases_input" "repos/$repository/releases?per_page=100" "$releases"

latest_release="$temporary_directory/latest-release.json"
if ! jq -e '
  if type != "array" then error("release response is not an array")
  elif length > 0 and (.[0] | type) == "array" then add
  else .
  end
  | map(select(.draft == false and .prerelease == false))
  | sort_by(.published_at // .created_at // "")
  | last
  | select(. != null)
' "$releases" >"$latest_release"; then
  fail "上流リポジトリに通常Releaseが見つかりませんでした。"
fi

latest_tag=$(jq -er '.tag_name | select(type == "string" and test("^v[0-9]+\\.[0-9]+\\.[0-9]+$"))' "$latest_release") ||
  fail "最新の通常ReleaseのタグがvX.Y.Z形式ではありません。"
latest_version=${latest_tag#v}
release_url=$(jq -er '
  .html_url
  | select(type == "string")
  | select(test("^https://github.com/KeishiS/mcp-authorization-server/releases/tag/[^[:space:]]+$"))
' "$latest_release") || fail "最新の通常ReleaseのURLが不正です。"

tag_reference="$temporary_directory/tag-reference.json"
copy_or_fetch "$tag_reference_input" "repos/$repository/git/ref/tags/$latest_tag" "$tag_reference"
tag_object_sha=$(jq -er '.object | select(.type == "tag") | .sha | select(test("^[0-9a-f]{40}$"))' "$tag_reference") ||
  fail "最新の通常Releaseが注釈付きタグを指していません: $latest_tag"

tag_object="$temporary_directory/tag-object.json"
copy_or_fetch "$tag_object_input" "repos/$repository/git/tags/$tag_object_sha" "$tag_object"
latest_revision=$(jq -er '.object | select(.type == "commit") | .sha | select(test("^[0-9a-f]{40}$"))' "$tag_object") ||
  fail "注釈付きタグから対象commitを解決できませんでした: $latest_tag"

if [[ -n "${GITHUB_OUTPUT:-}" ]]; then
  {
    printf 'pinned_version=%s\n' "$pinned_version"
    printf 'pinned_revision=%s\n' "$pinned_revision"
    printf 'latest_version=%s\n' "$latest_version"
    printf 'latest_revision=%s\n' "$latest_revision"
    printf 'latest_tag=%s\n' "$latest_tag"
    printf 'release_url=%s\n' "$release_url"
  } >>"$GITHUB_OUTPUT"
fi

if [[ "$pinned_version" == "$latest_version" && "$pinned_revision" == "$latest_revision" ]]; then
  echo "共有Authorization Serverは最新の通常Releaseと一致しています: $latest_tag $latest_revision"
  exit 0
fi

echo "共有Authorization Serverに固定内容との差異があります。" >&2
echo "固定中: v$pinned_version $pinned_revision" >&2
echo "上流最新: $latest_tag $latest_revision" >&2
exit 10
