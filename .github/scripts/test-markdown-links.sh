#!/usr/bin/env bash
set -euo pipefail

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
work_dir=$(mktemp -d)
trap 'rm -rf "$work_dir"' EXIT

mkdir -p "$work_dir/assets" "$work_dir/directory"
printf '{}\n' >"$work_dir/assets/contract.json"
printf 'image\n' >"$work_dir/assets/diagram.png"
cat >"$work_dir/good.md" <<'EOF'
[文書自身](good.md#section)
[JSON](assets/contract.json)
![画像](assets/diagram.png)
[directory](directory)
[外部](https://example.test/document)
[同一文書](#section)
EOF

bash "$script_dir/check-markdown-links.sh" "$work_dir/good.md"

for target in missing.md assets/missing.json assets/missing.png missing-directory; do
  printf '[存在しないリンク](%s)\n' "$target" >"$work_dir/broken.md"
  if bash "$script_dir/check-markdown-links.sh" "$work_dir/broken.md" >/dev/null 2>&1; then
    echo "存在しないリンク先を受理しました: $target" >&2
    exit 1
  fi
done
