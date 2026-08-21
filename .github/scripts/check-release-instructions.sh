#!/usr/bin/env bash
set -euo pipefail

# リリース手順と公開workflowが同じ状態遷移を表していることを検査します。
#   - 人が行う操作は、mainの先端SHAを指定したstable release workflowの実行だけです。
#   - tagの作成、Release Notesの転記、下書きの公開はworkflowだけが行います。
#   - workflowは、候補の再検証、注釈付きtagの作成、下書きのasset集合の検査、公開の順に進みます。
# 手順書へ手作業の公開操作が戻ってきた場合や、workflowから公開の要点が失われた場合に拒否します。

release_guide="${1:-docs/developer-guide/release.adoc}"
dispatch_workflow="${2:-.github/workflows/release-dispatch.yml}"
publish_workflow="${3:-.github/workflows/release-publish.yml}"

fail() {
  echo "$1" >&2
  exit 1
}

# 行頭の字下げを無視して、対象fileへその行がちょうど一つあることを求めます。
require_line() {
  local label=$1
  local file=$2
  local line=$3
  local count
  count=$(sed 's/^[[:space:]]*//' "$file" | grep -cFx -- "$line" || true)
  if [[ "$count" -ne 1 ]]; then
    fail "${label}に必要な記述が一つだけ現れていません: ${line}"
  fi
}

require_absent() {
  local label=$1
  local file=$2
  local pattern=$3
  local description=$4
  if grep -qE -- "$pattern" "$file"; then
    fail "${label}に手作業の公開操作が残っています: ${description}"
  fi
}

require_line "リリース手順" "$release_guide" \
  'gh workflow run release-dispatch.yml --ref main --field candidate_sha="$candidate_sha"'
require_line "リリース手順" "$release_guide" \
  'candidate_sha=$(git rev-parse upstream/main)'

require_absent "リリース手順" "$release_guide" \
  '(^|[[:space:]])git tag' "タグの手動作成"
require_absent "リリース手順" "$release_guide" \
  'git push .*(v\$|v[0-9])' "タグの手動push"
require_absent "リリース手順" "$release_guide" \
  'gh release (create|edit|upload)' "GitHub Releaseの手動操作"
require_absent "リリース手順" "$release_guide" \
  '--draft=false' "下書きの手動公開"

require_line "stable release workflow" "$dispatch_workflow" \
  'candidate_sha:'
require_line "stable release workflow" "$dispatch_workflow" \
  'bash .github/scripts/check-release-readiness.sh >>"$GITHUB_OUTPUT"'
require_line "stable release workflow" "$dispatch_workflow" \
  'run: bash .github/scripts/release-candidate.sh verify candidate "$CANDIDATE_SHA"'

require_line "公開workflow" "$publish_workflow" \
  'bash .github/scripts/release-candidate.sh verify candidate "$CANDIDATE_SHA"'
require_line "公開workflow" "$publish_workflow" \
  'subject-path: assets/*'
require_line "公開workflow" "$publish_workflow" \
  'gh api --method POST "repos/$GITHUB_REPOSITORY/git/refs" \'
require_line "公開workflow" "$publish_workflow" \
  'echo "下書きのasset集合が宣言と一致しません。" >&2'
require_line "公開workflow" "$publish_workflow" \
  'result="$(gh api --method PATCH "repos/$GITHUB_REPOSITORY/releases/$RELEASE_ID" -F draft=false)"'
# 公開の途中で失敗した場合に、中途半端な下書きを残さないための後始末です。
require_line "公開workflow" "$publish_workflow" \
  'if: failure()'
require_line "公開workflow" "$publish_workflow" \
  'gh api --method DELETE "repos/$GITHUB_REPOSITORY/releases/$RELEASE_ID"'

# binary cacheへのpushは公開の後段に置き、CPUごとに独立して再実行できるようにします。
require_line "stable release workflow" "$dispatch_workflow" \
  'needs: [readiness, publish]'
require_line "stable release workflow" "$dispatch_workflow" \
  'fail-fast: false'

# 公開の要点は、候補の再検証、tag作成、asset集合の検査、公開の順でなければなりません。
ordered=(
  'bash .github/scripts/release-candidate.sh verify candidate "$CANDIDATE_SHA"'
  'gh api --method POST "repos/$GITHUB_REPOSITORY/git/refs" \'
  'echo "下書きのasset集合が宣言と一致しません。" >&2'
  'result="$(gh api --method PATCH "repos/$GITHUB_REPOSITORY/releases/$RELEASE_ID" -F draft=false)"'
)
previous_line=0
for step in "${ordered[@]}"; do
  current_line=$(sed 's/^[[:space:]]*//' "$publish_workflow" | grep -nFx -- "$step" | head -n 1)
  current_line="${current_line%%:*}"
  if ((current_line <= previous_line)); then
    fail "公開workflowの操作順序が正しくありません: ${step}"
  fi
  previous_line="$current_line"
done

echo "候補の指定だけで公開へ進む手順と、workflowの公開順序を確認しました。"
