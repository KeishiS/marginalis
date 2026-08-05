#!/usr/bin/env bash
set -euo pipefail

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
script="$script_dir/classify-ci-change.sh"
temporary_directory=$(mktemp -d)
trap 'rm -rf "$temporary_directory"' EXIT

git -C "$temporary_directory" init --quiet
git -C "$temporary_directory" config user.name "Marginalis CI"
git -C "$temporary_directory" config user.email "ci@example.invalid"
printf '%s\n' '= 試験' >"$temporary_directory/README.adoc"
git -C "$temporary_directory" add README.adoc
git -C "$temporary_directory" commit --quiet -m base
base=$(git -C "$temporary_directory" rev-parse HEAD)

printf '%s\n' '= 文書の変更' >"$temporary_directory/README.adoc"
git -C "$temporary_directory" commit --quiet -am docs
docs_head=$(git -C "$temporary_directory" rev-parse HEAD)
actual=$(cd "$temporary_directory" && bash "$script" "$base" "$docs_head")
test "$actual" = true

mkdir -p "$temporary_directory/frontend"
printf '%s\n' 'export {};' >"$temporary_directory/frontend/example.ts"
git -C "$temporary_directory" add frontend/example.ts
git -C "$temporary_directory" commit --quiet -m code
code_head=$(git -C "$temporary_directory" rev-parse HEAD)
actual=$(cd "$temporary_directory" && bash "$script" "$docs_head" "$code_head")
test "$actual" = false

actual=$(cd "$temporary_directory" && bash "$script" "" "$code_head")
test "$actual" = false
actual=$(cd "$temporary_directory" && bash "$script" "0000000000000000000000000000000000000000" "$code_head")
test "$actual" = false
