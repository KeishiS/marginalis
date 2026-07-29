#!/usr/bin/env bash
set -euo pipefail

base="${1:-}"
head="${2:-HEAD}"
zero=0000000000000000000000000000000000000000
script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)

if [[ -z "$base" || "$base" == "$zero" ]] ||
  ! git cat-file -e "$base^{commit}" 2>/dev/null ||
  ! git cat-file -e "$head^{commit}" 2>/dev/null
then
  printf '%s\n' false
  exit 0
fi

git diff --name-only -z "$base" "$head" |
  bash "$script_dir/classify-docs-only.sh"
