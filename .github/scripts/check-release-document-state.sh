#!/usr/bin/env bash
set -euo pipefail

expected_version="${1:?expected version is required}"
acceptance_file="${2:?acceptance file is required}"
changelog_file="${3:?changelog file is required}"
release_tag="${MARGINALIS_RELEASE_TAG:-}"

if ! decision="$(
  awk '
    /^\* 公開判定: / {
      matches++
      if ($0 == "* 公開判定: 未判定") {
        decision = "未判定"
      } else if ($0 == "* 公開判定: 公開停止") {
        decision = "公開停止"
      } else if ($0 == "* 公開判定: 公開可") {
        decision = "公開可"
      } else {
        invalid = 1
      }
    }
    END {
      if (matches != 1 || invalid) {
        exit 1
      }
      print decision
    }
  ' "$acceptance_file"
)"; then
  echo "受入結果の公開判定が一意または有効ではありません: $acceptance_file" >&2
  exit 1
fi

heading="== $expected_version — "
if ! changelog_status="$(
  awk -v heading="$heading" '
    index($0, heading) == 1 {
      matches++
      status = substr($0, length(heading) + 1)
    }
    END {
      if (matches != 1) {
        exit 1
      }
      print status
    }
  ' "$changelog_file"
)"; then
  echo "対象バージョンの変更履歴見出しが一意ではありません: $changelog_file" >&2
  exit 1
fi

date_pattern='^[0-9]{4}-[0-9]{2}-[0-9]{2}$'
if [[ "$decision" == "公開可" ]]; then
  if [[ ! "$changelog_status" =~ $date_pattern ]]; then
    echo "公開可の受入結果には変更履歴の公開日が必要です: $changelog_file" >&2
    exit 1
  fi
elif [[ "$changelog_status" != "未公開" && ! "$changelog_status" =~ $date_pattern ]]; then
  echo "変更履歴の状態は未公開またはYYYY-MM-DD形式の日付でなければなりません: $changelog_file" >&2
  exit 1
fi

if [[ -n "$release_tag" ]]; then
  if [[ "$release_tag" != "v$expected_version" ]]; then
    echo "リリースタグとworkspaceの版が一致しません: $release_tag" >&2
    exit 1
  fi
  if [[ "$decision" != "公開可" ]]; then
    echo "リリースタグの検証には公開可の受入結果が必要です: $acceptance_file" >&2
    exit 1
  fi
fi
