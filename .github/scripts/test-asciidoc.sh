#!/usr/bin/env bash
set -euo pipefail

work_directory="$(mktemp -d)"
trap 'rm -rf "$work_directory"' EXIT

cp .adocweave.toml "$work_directory/.adocweave.toml"
mkdir -p "$work_directory/guide"

printf '%s\n' \
  '= 入口' \
  '' \
  'xref:guide/details.adoc#details[詳細]' \
  >"$work_directory/index.adoc"
printf '%s\n' \
  '= 詳細' \
  '' \
  '[#details]' \
  '== 検証対象' \
  '' \
  '参照先です。' \
  >"$work_directory/guide/details.adoc"

adocweave check \
  --fail-on warning \
  --local-targets \
  --project-root "$work_directory" \
  "$work_directory/index.adoc" >/dev/null
cargo run --quiet --locked -p marginalis-documentation -- \
  check-xrefs \
  --project-root "$work_directory" \
  "$work_directory/index.adoc" \
  "$work_directory/guide/details.adoc"

printf '%s\n' \
  '= 壊れた参照' \
  '' \
  'xref:guide/details.adoc#missing[存在しないID]' \
  >"$work_directory/broken.adoc"
if cargo run --quiet --locked -p marginalis-documentation -- \
  check-xrefs \
  --project-root "$work_directory" \
  "$work_directory/broken.adoc" \
  "$work_directory/guide/details.adoc" >/dev/null 2>&1; then
  echo "存在しない明示IDへのxrefを受理しました。" >&2
  exit 1
fi

printf '%s\n' \
  '= 壊れたinclude' \
  '' \
  'include::missing.adoc[]' \
  >"$work_directory/broken.adoc"
if adocweave check \
  --fail-on warning \
  --local-targets \
  --project-root "$work_directory" \
  "$work_directory/broken.adoc" >/dev/null 2>&1; then
  echo "存在しないinclude対象を受理しました。" >&2
  exit 1
fi

echo "AsciiDoc文書検査の回帰試験に成功しました。"
