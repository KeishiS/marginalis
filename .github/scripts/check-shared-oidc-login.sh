#!/usr/bin/env bash
set -euo pipefail

# 共有OIDCログインの2 crateは独立リポジトリで保守し、Marginalisは完全SHAを固定したGit依存として
# 取り込みます(ADR 0015)。この検査は、その固定が崩れていないことだけを確かめます。
# ID token検証の正しさや疑似IdPの動作は上流リポジトリのCIが検証します。

repository='https://github.com/KeishiS/oidc-browser-login.git'
core='oidc-browser-login'
testkit='oidc-browser-login-testkit'

temporary_directory=$(mktemp -d)
trap 'rm -rf "$temporary_directory"' EXIT

metadata="$temporary_directory/metadata.json"
metadata_input="${1:-}"
flake_file="${2:-flake.nix}"

if [[ -n "$metadata_input" ]]; then
  cp "$metadata_input" "$metadata"
else
  cargo metadata --locked --format-version 1 >"$metadata"
fi

fail() {
  echo "$1" >&2
  exit 1
}

# 1. Marginalis側の宣言をすべて集め、版・URL・revisionが一つに揃っていることを確かめます。
#    試験専用のtestkitはdev依存としてだけ宣言できます。
declarations="$temporary_directory/declarations"
jq -r --arg core "$core" --arg testkit "$testkit" '
  . as $metadata
  | $metadata.packages[]
  | select(.id as $id | $metadata.workspace_members | index($id))
  | .name as $consumer
  | .dependencies[]
  | select(.name == $core or .name == $testkit)
  | [$consumer, .name, (.kind // "normal"), .req, (.source // "")]
  | @tsv
' "$metadata" | sort >"$declarations"

if [[ ! -s "$declarations" ]]; then
  fail "共有OIDCログインへの依存宣言が見つかりません。"
fi

requirements=""
sources=""
while IFS=$'\t' read -r consumer name kind requirement source; do
  if [[ ! "$requirement" =~ ^=[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    fail "共有OIDCログインの版を完全一致で固定してください: $consumer -> $name = $requirement"
  fi
  if [[ ! "$source" =~ ^git\+${repository//./\\.}\?rev=[0-9a-f]{40}$ ]]; then
    fail "共有OIDCログインを40桁の完全SHA付きGit依存にしてください: $consumer -> $name ($source)"
  fi
  if [[ "$name" == "$testkit" && "$kind" != "dev" ]]; then
    fail "testkitはdev依存としてだけ宣言してください: $consumer -> $name ($kind)"
  fi
  requirements+="$requirement"$'\n'
  sources+="$source"$'\n'
done <"$declarations"

if [[ "$(printf '%s' "$requirements" | sort -u | wc -l)" -ne 1 ]]; then
  fail "2 crateの版指定が一致しません。ルートCargo.tomlで同じ版を指定してください。"
fi
if [[ "$(printf '%s' "$sources" | sort -u | wc -l)" -ne 1 ]]; then
  fail "2 crateのGit URLまたはrevisionが一致しません。同じcommitを指定してください。"
fi

declared_version="${requirements%%$'\n'*}"
declared_version="${declared_version#=}"
declared_source="${sources%%$'\n'*}"
declared_revision="${declared_source##*?rev=}"

# 2. 解決結果が宣言どおりであり、crateごとに1版だけであることを確かめます。
resolved="$temporary_directory/resolved"
jq -r --arg core "$core" --arg testkit "$testkit" '
  .packages[]
  | select(.name == $core or .name == $testkit)
  | [.name, .version, (.source // "path"), (.license // "")]
  | @tsv
' "$metadata" | sort >"$resolved"

for name in "$core" "$testkit"; do
  count=$(awk -F'\t' -v name="$name" '$1 == name { total += 1 } END { print total + 0 }' "$resolved")
  if [[ "$count" -ne 1 ]]; then
    fail "$name の解決結果が1件ではありません（$count 件）。単一のrevisionへ揃えてください。"
  fi
done

expected_source="git+${repository}?rev=${declared_revision}#${declared_revision}"
while IFS=$'\t' read -r name version source license; do
  if [[ "$source" == "path" ]]; then
    fail "$name がローカルpathから解決されています。独立リポジトリの固定revisionを使ってください。"
  fi
  if [[ "$version" != "$declared_version" ]]; then
    fail "$name の解決版が宣言と一致しません: $version != $declared_version"
  fi
  if [[ "$source" != "$expected_source" ]]; then
    fail "$name の取得元が宣言と一致しません: $source != $expected_source"
  fi
  if [[ "$license" != "MIT OR Apache-2.0" ]]; then
    fail "$name のライセンスがMIT OR Apache-2.0ではありません: $license"
  fi
done <"$resolved"

# 3. testkitが同じrevisionの中核へ依存していることを確かめます。
if ! jq -e --arg core "$core" --arg testkit "$testkit" --arg source "$expected_source" '
  . as $metadata
  | ($metadata.packages[] | select(.name == $testkit) | .id) as $testkit_id
  | ($metadata.packages[]
      | select(.name == $core and .source == $source)
      | .id) as $core_id
  | $metadata.resolve.nodes[]
  | select(.id == $testkit_id)
  | .deps
  | map(.pkg)
  | index($core_id)
' "$metadata" >/dev/null; then
  fail "$testkit が同じrevisionの $core へ依存していません。"
fi

# 4. marginalis-serviceのproduction依存から中核へ到達でき、testkitへは到達できないことを
#    確かめます。testkitの試験用鍵と疑似IdPをproduction buildへ含めないためです。
reachable="$temporary_directory/reachable"
jq -r --arg core "$core" --arg testkit "$testkit" '
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
  | select(.name == $core or .name == $testkit)
  | .name
' "$metadata" | sort -u >"$reachable"

if ! grep -qx "$core" "$reachable"; then
  fail "marginalis-serviceのproduction依存から $core へ到達できません。"
fi
if grep -qx "$testkit" "$reachable"; then
  fail "marginalis-serviceのproduction依存から $testkit へ到達できます。dev依存へ戻してください。"
fi

# 5. Nixのcargoハッシュが2 crateとも登録されていることを確かめます。
for name in "$core" "$testkit"; do
  if ! grep -q "\"${name}-${declared_version}\"" "$flake_file"; then
    fail "${flake_file}のcargoLock.outputHashesへ \"${name}-${declared_version}\" を追加してください。"
  fi
done

echo "共有OIDCログインの固定を確認しました: v${declared_version} ${declared_revision}"
