# v0.9.0（schema 9）のdatabaseからarchiveを取り出し、migrate-archiveを経て現行schemaへ
# 取り込めること、内容が失われないことを確かめる。
{
  pkgs,
  self,
  system,
  rustPlatform,
  adocweaveVersion,
}:
let
  marginalisV090Source = builtins.fetchTarball {
    url = "https://github.com/KeishiS/marginalis/archive/9a286ebbb0a86065138cc658e46628175ba876e2.tar.gz";
    sha256 = "sha256-ljexEdr0oaF/u5IiMpWj7W98tdyDthmDeotxfBhj2CM=";
  };
  marginalisV090AdocweaveConformanceCases = pkgs.fetchurl {
    url = "https://raw.githubusercontent.com/KeishiS/adocweave/778e9da4548f03ea8434677d50c819d7ce665809/fixtures/conformance/cases.json";
    hash = "sha256-OxHK8NobfmNN9pRj7B3qP94s1b2E26l5y5EQdMQq6aY=";
  };
  marginalisV090 = rustPlatform.buildRustPackage {
    pname = "marginalis";
    version = "0.9.0";
    src = marginalisV090Source;
    cargoLock = {
      lockFile = "${marginalisV090Source}/Cargo.lock";
      outputHashes = {
        "adocweave-0.11.0" = "sha256-1qCSy6eWSGhIxu1jsLFsRrX2OXNuYgnV6lmTwchGiT4=";
      };
    };
    cargoBuildFlags = [
      "--package"
      "marginalis-service"
      "--bin"
      "marginalis-service"
    ];
    preBuild = ''
      install -Dm444 ${marginalisV090AdocweaveConformanceCases} ../fixtures/conformance/cases.json
      # Archive CLIはWeb assetを使用しない。v0.9.0のinclude_bytes!に必要な
      # pathだけを用意し、移行checkで不要なfrontend buildを避ける。
      mkdir -p frontend/dist/assets
      touch frontend/dist/assets/{editor.js,editor.css,tex-svg.js,page.js}
    '';
    doCheck = false;
    installPhase = ''
      install -Dm755 target/${pkgs.stdenv.hostPlatform.rust.cargoShortTarget}/release/marginalis-service $out/bin/marginalis
    '';
  };
in
pkgs.runCommand "marginalis-schema9-archive-migration"
  {
    nativeBuildInputs = [
      pkgs.coreutils
      pkgs.jq
      pkgs.sqlite
    ];
  }
  ''
    export MARGINALIS_DATABASE_URL="sqlite:$PWD/schema9.sqlite"
    ${marginalisV090}/bin/marginalis export-archive --output "$PWD/empty.json"
    test "$(sqlite3 schema9.sqlite \
      'SELECT MAX(version) FROM schema_migrations')" = 9
    rm empty.json

    sqlite3 schema9.sqlite < ${./fixtures/schema9-seed.sql}
    sqlite3 -json schema9.sqlite \
      'SELECT note_id, creator_issuer, creator_subject, title, source,
              tags_json, created_at_ms, updated_at_ms, revision, deleted_at_ms
       FROM notes ORDER BY note_id' > schema9-notes.json
    sqlite3 -json schema9.sqlite \
      'SELECT source_note_id, target_note_id
       FROM note_references ORDER BY source_note_id, target_note_id' \
      > schema9-references.json
    sqlite3 -json schema9.sqlite \
      'SELECT note_id, issuer, subject, permission
       FROM note_acl ORDER BY note_id, issuer, subject' > schema9-acl.json

    ${marginalisV090}/bin/marginalis export-archive --output "$PWD/schema9.json"
    jq -e '
      .format == "marginalis-archive-7"
      and (.notes | length) == 2
      and (.note_acl | length) == 2
      and any(.notes[];
        .note_id == "019f0000-0000-7000-8000-000000000091"
        and .revision == 4 and .deleted_at_ms == null)
      and any(.notes[];
        .note_id == "019f0000-0000-7000-8000-000000000092"
        and .revision == 2 and .deleted_at_ms == 6000)
    ' schema9.json

    cp schema9.json schema9-original.json
    export MARGINALIS_DATABASE_URL="sqlite:$PWD/rejected.sqlite"
    ! ${self.packages.${system}.default}/bin/marginalis \
      import-archive --input "$PWD/schema9.json"
    test ! -e rejected.sqlite
    ${self.packages.${system}.default}/bin/marginalis \
      migrate-archive --input "$PWD/schema9.json" --output "$PWD/migrated-archive.json"
    cmp schema9.json schema9-original.json
    jq -e '
      .format == "marginalis-archive-17"
      and .adocweave_package_version == "${adocweaveVersion}"
      and .note_profile_version == 5
      and (.notes | length) == 2
      and (.note_acl | length) == 2
    ' migrated-archive.json
    # 移行はタグの文書属性を接頭辞付きの名前へ書き換える。
    jq -e '
      any(.notes[];
        .note_id == "019f0000-0000-7000-8000-000000000091"
        and (.source | contains(":marginalis-tags: 移行, 検証"))
        and (.source | contains(":tags:") | not))
    ' migrated-archive.json

    export MARGINALIS_DATABASE_URL="sqlite:$PWD/schema22.sqlite"
    ${self.packages.${system}.default}/bin/marginalis \
      import-archive --input "$PWD/migrated-archive.json"
    test "$(sqlite3 schema22.sqlite \
      'SELECT MAX(version) FROM schema_migrations')" = 22
    sqlite3 -json schema22.sqlite \
      'SELECT note_id, creator_issuer, creator_subject, title, source,
              tags_json, created_at_ms, updated_at_ms, revision, deleted_at_ms
       FROM notes ORDER BY note_id' > schema22-notes.json
    sqlite3 -json schema22.sqlite \
      'SELECT source_note_id, target_note_id
       FROM note_references ORDER BY source_note_id, target_note_id' \
      > schema22-references.json
    sqlite3 -json schema22.sqlite \
      'SELECT note_id, issuer, subject, permission
       FROM note_acl ORDER BY note_id, issuer, subject' > schema22-acl.json
    # 本文はタグの属性名だけが変わる。題名、タグ、時刻、revision、削除状態は
    # 移行前と一致しなければならない。書き出し方の違いを比較へ持ち込まないよう、
    # 両方を同じ整形で並べ直してから照合する。
    jq -S '[.[] | .source |= sub(":tags: "; ":marginalis-tags: ")]' \
      schema9-notes.json > schema9-notes-expected.json
    jq -S '.' schema22-notes.json > schema22-notes-normalized.json
    diff -u schema9-notes-expected.json schema22-notes-normalized.json
    cmp schema9-references.json schema22-references.json
    cmp schema9-acl.json schema22-acl.json
    test "$(sqlite3 schema22.sqlite \
      "SELECT COUNT(*) FROM sqlite_schema
       WHERE type = 'table' AND name IN
         ('mcp_clients', 'mcp_authorization_codes',
          'mcp_access_tokens', 'mcp_refresh_tokens',
          'mcp_principal_scope_ceilings',
          'mcp_client_scope_ceilings',
          'mcp_client_authorizations',
          'math_macro_settings',
          'bibliography_import_sources',
          'bibliography_import_links')")" = 10

    ${self.packages.${system}.default}/bin/marginalis \
      export-archive --output "$PWD/roundtrip-archive.json"
    cmp migrated-archive.json roundtrip-archive.json
    ${self.packages.${system}.default}/bin/marginalis \
      verify-restore --input "$PWD/roundtrip-archive.json"
    touch $out
  ''
