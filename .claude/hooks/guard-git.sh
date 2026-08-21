#!/usr/bin/env bash
# Claude CodeのPreToolUse hook。規約で禁止しているgit操作を実行前に拒否します。
# stdinのJSONからBashコマンドを読み取り、違反時はexit 2(block)で理由をstderrへ返します。
# 対象: mainへの直接push、force push、リリースタグ(v*)のpush。
set -euo pipefail

command=$(jq -r '.tool_input.command // empty' 2>/dev/null || true)
[[ -n "$command" ]] || exit 0

deny() {
  echo "$1" >&2
  exit 2
}

# 連結コマンドを区切りごとに分け、gitのpushを含む区切りだけを検査します。
while IFS= read -r segment; do
  grep -qE '(^|[[:space:]])git([[:space:]]|$)' <<<"$segment" || continue
  grep -qE '(^|[[:space:]])push([[:space:]]|$)' <<<"$segment" || continue
  seen_push=0
  delete_seen=0
  for word in $segment; do
    if [[ "$seen_push" == 0 ]]; then
      [[ "$word" == push ]] && seen_push=1
      continue
    fi
    case "$word" in
      -f | --force | --force-with-lease | --force-with-lease=* | --force-if-includes)
        deny "force pushは禁止されています。mainを作業ブランチへmergeして競合を解消してください。"
        ;;
      main | +main | HEAD:main | refs/heads/main | *:main | *:refs/heads/main)
        deny "mainへの直接pushは禁止されています。作業ブランチを作成しPull Requestを使ってください。"
        ;;
      --delete | -d)
        delete_seen=1
        ;;
      :v[0-9]* | :refs/tags/v[0-9]*)
        deny "リリースタグの削除pushは禁止されています(protect-release-tags)。"
        ;;
      v[0-9]* | refs/tags/v[0-9]*)
        if [[ "$delete_seen" == 1 ]]; then
          deny "リリースタグの削除pushは禁止されています(protect-release-tags)。"
        fi
        deny "リリースタグは公開workflowだけが作成します。mainの先端SHAを指定してrelease-dispatchを実行してください。"
        ;;
    esac
  done
done < <(tr ';|&' '\n' <<<"$command")

exit 0
