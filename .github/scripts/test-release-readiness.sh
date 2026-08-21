#!/usr/bin/env bash
set -euo pipefail

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
readiness="$script_dir/check-release-readiness.sh"

tip="0123456789abcdef0123456789abcdef01234567"
older="89abcdef0123456789abcdef0123456789abcdef"

input() {
  jq -n --arg tip "$tip" '{
    ref: "refs/heads/main",
    candidateSha: $tip,
    dispatchSha: $tip,
    version: "1.2.3",
    tagExists: false,
    releaseExists: false,
    runs: [
      {id: 11, headSha: $tip, headBranch: "main", event: "push", candidateConclusion: "success"},
      {id: 9, headSha: $tip, headBranch: "main", event: "push", candidateConclusion: "success"}
    ]
  }'
}

accept() {
  local description=$1
  local expected=$2
  local actual
  if ! actual="$(input | jq "$3" | bash "$readiness" 2>/dev/null)"; then
    echo "受理すべき状態を拒否しました: $description" >&2
    exit 1
  fi
  if [[ "$actual" != "$expected" ]]; then
    echo "判定結果が想定と異なります: $description" >&2
    diff -u <(printf '%s\n' "$expected") <(printf '%s\n' "$actual") >&2 || true
    exit 1
  fi
}

reject() {
  local description=$1
  if input | jq "$2" | bash "$readiness" >/dev/null 2>&1; then
    echo "受理してはいけない状態を受理しました: $description" >&2
    exit 1
  fi
}

# 先端のcommitに成功した候補があれば、いちばん新しいrunを公開対象に選びます。
accept "先端の候補" "$(printf 'candidate_sha=%s\nrun_id=11\ntag=v1.2.3\n' "$tip")" '.'

# mainが進み、指定commitが先端でなくなった場合
reject "先端でないcommitの指定" ".candidateSha = \"$older\""

# mainの先端に対して候補作成が成功していない場合
reject "候補の不在" '.runs = []'
reject "候補の失敗" '.runs[].candidateConclusion = "failure"'
reject "候補作成の省略" '.runs[].candidateConclusion = "skipped"'
reject "候補作成jobの不在" '.runs[].candidateConclusion = "missing"'
reject "別commitの候補" ".runs[].headSha = \"$older\""
reject "main以外の候補" '.runs[].headBranch = "agent/ci/example"'
reject "push以外の候補" '.runs[].event = "workflow_dispatch"'

# すでに公開済み、または公開の途中で作られたtagとReleaseがある場合
reject "tagの重複" '.tagExists = true'
reject "Releaseの重複" '.releaseExists = true'

# main以外のrefからdispatchした場合
reject "main以外からの実行" '.ref = "refs/heads/agent/ci/example"'

# 入力の形式が不正な場合
reject "commit形式" '.candidateSha = "main"'
reject "版の形式" '.version = "1.2"'
reject "先端の欠落" '.dispatchSha = ""'

echo "stable release可否判定の自己テストに成功しました。"
