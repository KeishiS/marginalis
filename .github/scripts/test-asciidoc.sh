#!/usr/bin/env bash
set -euo pipefail

work_dir="$(mktemp -d)"
trap 'rm -rf "$work_dir"' EXIT

cp .adocweave.toml "$work_dir/.adocweave.toml"
mkdir -p "$work_dir/guide"

printf '%s\n' \
  '= 入口' \
  '' \
  'xref:guide/details.adoc#details[詳細]' \
  >"$work_dir/index.adoc"
printf '%s\n' \
  '= 詳細' \
  '' \
  '[#details]' \
  '== 検証対象' \
  '' \
  '参照先です。' \
  >"$work_dir/guide/details.adoc"

adocweave check \
  --fail-on warning \
  --local-targets \
  --project-root "$work_dir" \
  "$work_dir/index.adoc" >/dev/null
cargo run --quiet --locked -p marginalis-documentation -- \
  check-xrefs \
  --project-root "$work_dir" \
  "$work_dir/index.adoc" \
  "$work_dir/guide/details.adoc"

printf '%s\n' \
  '= 壊れた参照' \
  '' \
  'xref:guide/details.adoc#missing[存在しないID]' \
  >"$work_dir/broken.adoc"
if cargo run --quiet --locked -p marginalis-documentation -- \
  check-xrefs \
  --project-root "$work_dir" \
  "$work_dir/broken.adoc" \
  "$work_dir/guide/details.adoc" >/dev/null 2>&1; then
  echo "存在しない明示IDへのxrefを受理しました。" >&2
  exit 1
fi

printf '%s\n' \
  '= 壊れたinclude' \
  '' \
  'include::missing.adoc[]' \
  >"$work_dir/broken.adoc"
if adocweave check \
  --fail-on warning \
  --local-targets \
  --project-root "$work_dir" \
  "$work_dir/broken.adoc" >/dev/null 2>&1; then
  echo "存在しないinclude対象を受理しました。" >&2
  exit 1
fi

echo "AsciiDoc文書検査の回帰試験に成功しました。"
