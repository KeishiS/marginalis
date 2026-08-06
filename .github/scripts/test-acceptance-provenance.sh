#!/usr/bin/env bash
set -euo pipefail

script_directory="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
work_directory="$(mktemp -d)"
trap 'rm -rf "$work_directory"' EXIT

acceptance_file="$work_directory/v1.2.3.adoc"
sha_a='1111111111111111111111111111111111111111'
sha_b='2222222222222222222222222222222222222222'
sha_c='3333333333333333333333333333333333333333'
sha_d='4444444444444444444444444444444444444444'

decision='公開可'

write_fixture() {
  local target="${1:-}"
  local human_commit="${2:-$sha_a}"
  local human_tree="${3:-$sha_b}"
  local release_commit="${4:-}"
  local release_tree="${5:-}"
  if [[ -z "$target" ]]; then
    target='refs/tags/v1.2.3^{commit}'
  fi
  {
    printf '* 人手受入対象コミット: ``%s``\n' "$human_commit"
    printf '* 人手受入対象tree: ``%s``\n' "$human_tree"
    printf '* 最終リリース対象: ``%s``\n' "$target"
    if [[ -n "$release_commit" ]]; then
      printf '* 最終リリースコミット: ``%s``\n' "$release_commit"
    fi
    if [[ -n "$release_tree" ]]; then
      printf '* 最終リリースtree: ``%s``\n' "$release_tree"
    fi
    printf '* 公開判定: %s\n' "$decision"
  } >"$acceptance_file"
}

expect_success() {
  local name="$1"
  if ! bash "$script_directory/check-acceptance-provenance.sh" \
    1.2.3 "$acceptance_file" >/dev/null; then
    echo "成功すべき受入証跡を拒否しました: $name" >&2
    exit 1
  fi
}

expect_failure() {
  local name="$1"
  if bash "$script_directory/check-acceptance-provenance.sh" \
    1.2.3 "$acceptance_file" >/dev/null 2>&1; then
    echo "失敗すべき受入証跡を受理しました: $name" >&2
    exit 1
  fi
}

write_fixture
expect_success 'タグ作成前の記録'

write_fixture 'refs/tags/v1.2.3^{commit}' "$sha_a" "$sha_b" "$sha_c" "$sha_d"
expect_success '公開後のcommitとtreeを含む記録'

write_fixture 'refs/tags/v9.9.9^{commit}'
expect_failure '版と異なるタグ参照'

write_fixture 'refs/tags/v1.2.3^{commit}' short "$sha_b"
expect_failure '短い人手受入commit'

write_fixture 'refs/tags/v1.2.3^{commit}' "$sha_a" "$sha_b" "$sha_c"
expect_failure '最終リリースtreeの欠落'

write_fixture
printf '* 人手受入対象コミット: ``%s``\n' "$sha_a" >>"$acceptance_file"
expect_failure '重複した人手受入commit'

# 人手受入の対象コミットは配備後にしか決まらないため、準備段階では未確定を許す。
decision='未判定'
write_fixture 'refs/tags/v1.2.3^{commit}' 未確定 未確定
expect_success '受入前の未確定な対象'

write_fixture 'refs/tags/v1.2.3^{commit}' 未確定 "$sha_b"
expect_success '受入前に片方だけ確定した対象'

write_fixture 'refs/tags/v1.2.3^{commit}' short "$sha_b"
expect_failure '受入前でも短いcommit'

decision='公開可'
write_fixture 'refs/tags/v1.2.3^{commit}' 未確定 未確定
expect_failure '公開可なのに未確定な対象'

decision='公開停止'
write_fixture 'refs/tags/v1.2.3^{commit}' 未確定 未確定
expect_success '公開停止で未確定な対象'

decision='公開可'
write_fixture
sed -i '/^\* 公開判定: /d' "$acceptance_file"
expect_failure '公開判定の欠落'

echo "受入証跡の回帰試験に成功しました。"
