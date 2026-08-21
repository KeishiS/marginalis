#!/usr/bin/env bash
set -euo pipefail

# stable releaseの公開可否を、GitHubから読み取った状態だけで判定します。標準入力へ次の形の
# JSONを与えると、公開する候補を一つに確定して`名前=値`の行を標準出力へ書きます。
#
#   {
#     "ref": "refs/heads/main",        dispatchを実行したref
#     "candidateSha": "<40桁>",        公開対象として人が指定したcommit
#     "dispatchSha": "<40桁>",         dispatch時点のmainの先端
#     "version": "0.48.0",             release-manifest.jsonのpackageVersion
#     "tagExists": false,              同じ版のtagの有無
#     "releaseExists": false,          同じ版のGitHub Releaseの有無
#     "runs": [                        候補を作ったworkflow runの一覧
#       {"id": 1, "headSha": "<40桁>", "headBranch": "main", "event": "push",
#        "candidateConclusion": "success"}
#     ]
#
# candidateConclusionは、そのrunのrelease-candidate jobの結果です。run全体ではなくこのjobを
# 見るため、binary cacheへのpushなど公開内容に影響しないjobの失敗では公開が止まりません。
#   }
#
# 判定は読み取りだけで行い、tagやReleaseの作成は行いません。指定commitがmainの先端でない、
# 同じcommitの成功した候補がない、すでにtagまたはReleaseがある場合は、何も変更せず失敗します。

fail() {
  echo "$1" >&2
  exit 1
}

input="$(mktemp)"
trap 'rm -f "$input"' EXIT
cat >"$input"

jq -e . "$input" >/dev/null 2>&1 || fail "公開可否の入力をJSONとして読めません。"

ref="$(jq -r '.ref // ""' "$input")"
[[ "$ref" == "refs/heads/main" ]] ||
  fail "stable releaseはmainからだけ実行できます: ${ref}"

candidate_sha="$(jq -r '.candidateSha // ""' "$input")"
[[ "$candidate_sha" =~ ^[0-9a-f]{40}$ ]] ||
  fail "候補のcommitは小文字40桁で指定してください: ${candidate_sha}"

dispatch_sha="$(jq -r '.dispatchSha // ""' "$input")"
[[ "$dispatch_sha" =~ ^[0-9a-f]{40}$ ]] ||
  fail "dispatch時点のmainの先端を取得できません: ${dispatch_sha}"

# mainが進んだあとに古いcommitを公開しないように、指定commitが先端であることを要求します。
[[ "$candidate_sha" == "$dispatch_sha" ]] ||
  fail "指定したcommitがmainの先端ではありません: 指定=${candidate_sha}, 先端=${dispatch_sha}"

version="$(jq -r '.version // ""' "$input")"
[[ "$version" =~ ^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$ ]] ||
  fail "公開する版がMAJOR.MINOR.PATCH形式ではありません: ${version}"
tag="v${version}"

[[ "$(jq -r '.tagExists // false' "$input")" == false ]] ||
  fail "tagはすでに存在します。公開後の修正は新しいpatch版で行ってください: ${tag}"
[[ "$(jq -r '.releaseExists // false' "$input")" == false ]] ||
  fail "GitHub Releaseはすでに存在します: ${tag}"

run_id="$(jq -r --arg sha "$candidate_sha" '
  [.runs // [] | .[]
    | select(.headSha == $sha and .headBranch == "main" and .event == "push"
        and .candidateConclusion == "success")
    | .id]
  | max // ""
' "$input")"
[[ -n "$run_id" && "$run_id" != null ]] ||
  fail "同じcommitで成功したmainの候補作成runがありません: ${candidate_sha}"

printf 'candidate_sha=%s\nrun_id=%s\ntag=%s\n' "$candidate_sha" "$run_id" "$tag"
