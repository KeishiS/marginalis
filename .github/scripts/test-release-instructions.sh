#!/usr/bin/env bash
set -euo pipefail

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
work_dir=$(mktemp -d)
trap 'rm -rf "$work_dir"' EXIT

build_tree() {
  local root=$1
  mkdir -p "$root"
  cat >"$root/release.adoc" <<'EOF'
= リリース手順

. mainの先端を取得し、そのSHAを指定して公開workflowを実行します。

[source,sh]
----
git fetch upstream main
candidate_sha=$(git rev-parse upstream/main)
gh workflow run release-dispatch.yml --ref main --field candidate_sha="$candidate_sha"
----

. workflowの成功と、公開されたReleaseのURLを確認します。
EOF

  cat >"$root/release-dispatch.yml" <<'EOF'
name: Stable release

on:
  workflow_dispatch:
    inputs:
      candidate_sha:
        required: true
        type: string

jobs:
  readiness:
    steps:
      - id: readiness
        run: |
          jq -n '{}' |
            bash .github/scripts/check-release-readiness.sh >>"$GITHUB_OUTPUT"
  reuse-candidate:
    steps:
      - name: Verify the reused candidate
        run: bash .github/scripts/release-candidate.sh verify candidate "$CANDIDATE_SHA"
  binary-cache:
    needs: [readiness, publish]
    strategy:
      fail-fast: false
      matrix:
        include:
          - runner: ubuntu-24.04
EOF

  cat >"$root/release-publish.yml" <<'EOF'
name: Release publish

jobs:
  publish:
    steps:
      - name: Verify the immutable release input
        run: |
          bash .github/scripts/release-candidate.sh verify candidate "$CANDIDATE_SHA"
      - name: Attest the public assets
        with:
          subject-path: assets/*
      - name: Create the immutable annotated tag
        run: |
          gh api --method POST "repos/$GITHUB_REPOSITORY/git/refs" \
            -f ref="refs/tags/$RELEASE_TAG" >/dev/null
      - name: Create and verify the private draft
        run: |
          if [ "$expected" != "$actual" ]; then
            echo "下書きのasset集合が宣言と一致しません。" >&2
            exit 1
          fi
      - name: Publish the release
        run: |
          result="$(gh api --method PATCH "repos/$GITHUB_REPOSITORY/releases/$RELEASE_ID" -F draft=false)"
          test "$(jq -r .draft <<<"$result")" = false
      - name: Remove the incomplete draft
        if: failure()
        run: |
          gh api --method DELETE "repos/$GITHUB_REPOSITORY/releases/$RELEASE_ID"
EOF
}

check() {
  bash "$script_dir/check-release-instructions.sh" \
    "$1/release.adoc" "$1/release-dispatch.yml" "$1/release-publish.yml"
}

reject() {
  local description=$1
  if check "$work_dir/bad" >/dev/null 2>&1; then
    echo "受理してはいけない状態を受理しました: $description" >&2
    exit 1
  fi
}

rebuild() {
  rm -rf "$work_dir/bad"
  build_tree "$work_dir/bad"
}

build_tree "$work_dir/good"
check "$work_dir/good" >/dev/null

# 手順から候補SHAの指定が失われた場合
rebuild
sed -i '/candidate_sha=\$(git rev-parse upstream\/main)/d' "$work_dir/bad/release.adoc"
reject "候補SHAの取得手順の欠落"

rebuild
sed -i '/gh workflow run release-dispatch.yml/d' "$work_dir/bad/release.adoc"
reject "公開workflowの実行手順の欠落"

# 手作業の公開操作が手順書へ戻ってきた場合
rebuild
printf 'git tag -a "$release_tag" -m "Marginalis $release_tag"\n' >>"$work_dir/bad/release.adoc"
reject "タグの手動作成"

rebuild
printf 'git push upstream v0.48.0\n' >>"$work_dir/bad/release.adoc"
reject "タグの手動push"

rebuild
printf 'gh release create "$release_tag" --draft\n' >>"$work_dir/bad/release.adoc"
reject "GitHub Releaseの手動作成"

rebuild
printf 'gh release edit "$release_tag" --draft=false\n' >>"$work_dir/bad/release.adoc"
reject "下書きの手動公開"

# workflowから公開の要点が失われた場合
rebuild
sed -i '/check-release-readiness.sh/d' "$work_dir/bad/release-dispatch.yml"
reject "公開可否判定の欠落"

rebuild
sed -i '/release-candidate.sh verify/d' "$work_dir/bad/release-publish.yml"
reject "公開直前の候補再検証の欠落"

rebuild
sed -i '/subject-path: assets/d' "$work_dir/bad/release-publish.yml"
reject "attestationの欠落"

rebuild
sed -i '/git\/refs/d' "$work_dir/bad/release-publish.yml"
reject "タグ作成の欠落"

rebuild
sed -i '/下書きのasset集合が宣言と一致しません。/d' "$work_dir/bad/release-publish.yml"
reject "asset集合の検査の欠落"

rebuild
sed -i '/-F draft=false/d' "$work_dir/bad/release-publish.yml"
reject "公開手順の欠落"

rebuild
sed -i '/if: failure()/d' "$work_dir/bad/release-publish.yml"
reject "失敗時の下書き削除の欠落"

rebuild
sed -i '/--method DELETE/d' "$work_dir/bad/release-publish.yml"
reject "下書き削除操作の欠落"

# binary cacheのpushが公開の前段に置かれたり、片方の失敗で他方も止まる場合
rebuild
sed -i 's/^    needs: \[readiness, publish\]$/    needs: [readiness]/' "$work_dir/bad/release-dispatch.yml"
reject "公開前のbinary cache push"

rebuild
sed -i '/fail-fast: false/d' "$work_dir/bad/release-dispatch.yml"
reject "CPUごとの独立再実行の欠落"

# 候補を検証する前にタグを作る順序になった場合
rebuild
cat >"$work_dir/bad/release-publish.yml" <<'EOF'
name: Release publish

jobs:
  publish:
    steps:
      - name: Create the immutable annotated tag
        run: |
          gh api --method POST "repos/$GITHUB_REPOSITORY/git/refs" \
            -f ref="refs/tags/$RELEASE_TAG" >/dev/null
      - name: Verify the immutable release input
        run: |
          bash .github/scripts/release-candidate.sh verify candidate "$CANDIDATE_SHA"
      - name: Attest the public assets
        with:
          subject-path: assets/*
      - name: Create and verify the private draft
        run: |
          echo "下書きのasset集合が宣言と一致しません。" >&2
      - name: Publish the release
        run: |
          result="$(gh api --method PATCH "repos/$GITHUB_REPOSITORY/releases/$RELEASE_ID" -F draft=false)"
      - name: Remove the incomplete draft
        if: failure()
        run: |
          gh api --method DELETE "repos/$GITHUB_REPOSITORY/releases/$RELEASE_ID"
EOF
reject "候補の検証より前のタグ作成"

echo "リリース手順検査の自己テストに成功しました。"
