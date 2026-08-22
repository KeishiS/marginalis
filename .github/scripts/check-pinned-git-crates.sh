#!/usr/bin/env bash
set -euo pipefail

# 独立リポジトリで保守しているcrate群を、完全SHAを固定したGit依存として取り込めているかを
# 検査します。共有Authorization Server(ADR 0012)と共有OIDCログイン(ADR 0015)の両方を、
# 同じ規則でこのscriptが扱います。crate内部の正しさは上流リポジトリのCIが検証します。
#
# 使用方法:
#   check-pinned-git-crates.sh --label ラベル --repository URL \
#     --crate 中核crate [--crate 併用crate]... [--dev-crate 試験専用crate]... \
#     [--forbidden-production-feature feature] [--metadata file] [--flake file]
#
# 最初の--crateを中核crateとして扱い、ほかのcrateが同じrevisionの中核へ依存していることを
# 確かめます。--crateはproduction依存から到達できること、--dev-crateはdev依存としてだけ
# 宣言され、production依存から到達できないことを求めます。

label=''
repository=''
production_crates=()
dev_crates=()
forbidden_feature=''
metadata_input=''
flake_file='flake.nix'

while [[ $# -gt 0 ]]; do
  case "$1" in
    --label)
      label=${2:-}
      shift 2
      ;;
    --repository)
      repository=${2:-}
      shift 2
      ;;
    --crate)
      production_crates+=("${2:-}")
      shift 2
      ;;
    --dev-crate)
      dev_crates+=("${2:-}")
      shift 2
      ;;
    --forbidden-production-feature)
      forbidden_feature=${2:-}
      shift 2
      ;;
    --metadata)
      metadata_input=${2:-}
      shift 2
      ;;
    --flake)
      flake_file=${2:-}
      shift 2
      ;;
    *)
      echo "未対応の引数です: $1" >&2
      exit 2
      ;;
  esac
done

fail() {
  echo "$1" >&2
  exit 1
}

[[ -n "$label" ]] || fail "--labelを指定してください。"
[[ -n "$repository" ]] || fail "--repositoryを指定してください。"
[[ "${#production_crates[@]}" -gt 0 ]] || fail "--crateを一つ以上指定してください。"

core="${production_crates[0]}"
all_crates=("${production_crates[@]}" "${dev_crates[@]}")

temporary_directory=$(mktemp -d)
trap 'rm -rf "$temporary_directory"' EXIT

metadata="$temporary_directory/metadata.json"
if [[ -n "$metadata_input" ]]; then
  cp "$metadata_input" "$metadata"
else
  cargo metadata --locked --format-version 1 >"$metadata"
fi

names_json=$(printf '%s\n' "${all_crates[@]}" | jq -R . | jq -sc .)
dev_names_json=$(printf '%s\n' "${dev_crates[@]+"${dev_crates[@]}"}" |
  jq -R 'select(length > 0)' | jq -sc .)

# 1. 宣言された版・取得元・依存種別を確かめ、crate間で揃っていることを求めます。
declarations="$temporary_directory/declarations"
jq -r --argjson names "$names_json" '
  . as $metadata
  | $metadata.packages[]
  | select(.id as $id | $metadata.workspace_members | index($id))
  | .name as $consumer
  | .dependencies[]
  | select(.name as $name | $names | index($name))
  | [$consumer, .name, (.kind // "normal"), .req, (.source // ""),
     ((.features // []) | sort | join(","))]
  | @tsv
' "$metadata" | sort >"$declarations"

[[ -s "$declarations" ]] || fail "${label}への依存宣言が見つかりません。"

requirements=''
sources=''
while IFS=$'\t' read -r consumer name kind requirement source features; do
  if [[ ! "$requirement" =~ ^=[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    fail "${label}の版を完全一致で固定してください: $consumer -> $name = $requirement"
  fi
  if [[ ! "$source" =~ ^git\+${repository//./\\.}\?rev=[0-9a-f]{40}$ ]]; then
    fail "${label}を40桁の完全SHA付きGit依存にしてください: $consumer -> $name ($source)"
  fi
  if printf '%s\n' "${dev_crates[@]+"${dev_crates[@]}"}" | grep -qx "$name" &&
    [[ "$kind" != "dev" ]]; then
    fail "${name}はdev依存としてだけ宣言してください: $consumer -> $name ($kind)"
  fi
  if [[ -n "$forbidden_feature" && "$kind" != "dev" &&
    ",$features," == *",$forbidden_feature,"* ]]; then
    fail "${forbidden_feature} featureをproduction依存で有効にしないでください: $consumer -> $name"
  fi
  requirements+="$requirement"$'\n'
  sources+="$source"$'\n'
done <"$declarations"

if [[ "$(printf '%s' "$requirements" | sort -u | wc -l)" -ne 1 ]]; then
  fail "${label}のcrate間で版指定が一致しません。ルートCargo.tomlで同じ版を指定してください。"
fi
if [[ "$(printf '%s' "$sources" | sort -u | wc -l)" -ne 1 ]]; then
  fail "${label}のcrate間でGit URLまたはrevisionが一致しません。同じcommitを指定してください。"
fi

declared_version="${requirements%%$'\n'*}"
declared_version="${declared_version#=}"
declared_source="${sources%%$'\n'*}"
declared_revision="${declared_source##*?rev=}"
expected_source="git+${repository}?rev=${declared_revision}#${declared_revision}"

# 2. 解決結果が宣言どおりで、crateごとに1版だけであることを確かめます。
resolved="$temporary_directory/resolved"
jq -r --argjson names "$names_json" '
  .packages[]
  | select(.name as $name | $names | index($name))
  | [.name, .version, (.source // "path"), (.license // "")]
  | @tsv
' "$metadata" | sort >"$resolved"

for name in "${all_crates[@]}"; do
  count=$(awk -F'\t' -v name="$name" '$1 == name { total += 1 } END { print total + 0 }' "$resolved")
  if [[ "$count" -ne 1 ]]; then
    fail "${name}の解決結果が1件ではありません（${count}件）。単一のrevisionへ揃えてください。"
  fi
done

while IFS=$'\t' read -r name version source license; do
  if [[ "$source" == "path" ]]; then
    fail "${name}がローカルpathから解決されています。独立リポジトリの固定revisionを使ってください。"
  fi
  if [[ "$version" != "$declared_version" ]]; then
    fail "${name}の解決版が宣言と一致しません: $version != $declared_version"
  fi
  if [[ "$source" != "$expected_source" ]]; then
    fail "${name}の取得元が宣言と一致しません: $source != $expected_source"
  fi
  if [[ "$license" != "MIT OR Apache-2.0" ]]; then
    fail "${name}のライセンスがMIT OR Apache-2.0ではありません: $license"
  fi
done <"$resolved"

# 3. 中核以外のcrateが、同じrevisionの中核へ依存していることを確かめます。
for name in "${all_crates[@]}"; do
  [[ "$name" != "$core" ]] || continue
  if ! jq -e --arg core "$core" --arg companion "$name" --arg source "$expected_source" '
    . as $metadata
    | ($metadata.packages[] | select(.name == $companion) | .id) as $companion_id
    | ($metadata.packages[]
        | select(.name == $core and .source == $source)
        | .id) as $core_id
    | $metadata.resolve.nodes[]
    | select(.id == $companion_id)
    | .deps
    | map(.pkg)
    | index($core_id)
  ' "$metadata" >/dev/null; then
    fail "${name}が同じrevisionの${core}へ依存していません。"
  fi
done

# 4. marginalis-serviceのproduction依存グラフから、到達してよいcrateだけへ到達することを
#    確かめます。試験専用crateの鍵や疑似IdPをproduction buildへ含めないためです。
reachable="$temporary_directory/reachable"
jq -r --argjson names "$names_json" '
  . as $metadata
  | ($metadata.packages[] | select(.name == "marginalis-service") | .id) as $root
  | ($metadata.resolve.nodes
      | map({key: .id, value: [.deps[] | select(.dep_kinds | map(.kind) | index(null)) | .pkg]})
      | from_entries) as $graph
  | def closure($pending; $seen):
      if ($pending | length) == 0 then $seen
      else
        [$pending[] | select(. as $id | ($seen | index($id) | not))] as $new
        | closure([$new[] as $id | $graph[$id][]?]; $seen + $new)
      end;
    closure([$root]; [])[] as $id
  | $metadata.packages[]
  | select(.id == $id)
  | select(.name as $name | $names | index($name))
  | .name
' "$metadata" | sort -u >"$reachable"

for name in "${production_crates[@]}"; do
  if ! grep -qx "$name" "$reachable"; then
    fail "marginalis-serviceのproduction依存から${name}へ到達できません。"
  fi
done
for name in "${dev_crates[@]+"${dev_crates[@]}"}"; do
  if grep -qx "$name" "$reachable"; then
    fail "marginalis-serviceのproduction依存から${name}へ到達できます。dev依存へ戻してください。"
  fi
done

# 5. Nixのcargoハッシュが各crateとも登録されていることを確かめます。
for name in "${all_crates[@]}"; do
  if ! grep -q "\"${name}-${declared_version}\"" "$flake_file"; then
    fail "${flake_file}のcargoLock.outputHashesへ \"${name}-${declared_version}\" を追加してください。"
  fi
done

# 6. production buildで禁止featureが有効にならないことを確かめます。cargo treeが必要なため、
#    metadataを外から与えた検査では省略します。
if [[ -z "$metadata_input" && -n "$forbidden_feature" ]]; then
  enabled=$(
    cargo tree --package marginalis-service --edges normal --prefix none --format '{p} {f}' |
      awk -v names="$(printf '%s ' "${all_crates[@]}")" '
        { if (index(" " names " ", " " $1 " ") > 0) { $1 = ""; $2 = ""; $3 = ""; print } }
      ' |
      tr ',' '\n' |
      tr -d ' ' |
      sort -u
  )
  if printf '%s\n' "$enabled" | grep -qx "$forbidden_feature"; then
    fail "production buildで${forbidden_feature} featureが有効になっています。"
  fi
fi

echo "${label}の固定を確認しました: v${declared_version} ${declared_revision}"
