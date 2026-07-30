{
  description = "Development environment for Marginalis";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  inputs.rust-overlay = {
    url = "github:oxalica/rust-overlay";
    inputs.nixpkgs.follows = "nixpkgs";
  };

  outputs =
    {
      self,
      nixpkgs,
      rust-overlay,
      ...
    }:
    let
      systems = [
        "aarch64-darwin"
        "aarch64-linux"
        "x86_64-linux"
      ];
      version = (builtins.fromTOML (builtins.readFile ./Cargo.toml)).workspace.package.version;
      forAllSystems = nixpkgs.lib.genAttrs systems;
      pkgsFor =
        system:
        import nixpkgs {
          inherit system;
          overlays = [ rust-overlay.overlays.default ];
        };
      # AdocWeave v0.19.0 が要求する Rust 1.97.1 を確定的にピンする。
      rustToolchainFor =
        pkgs:
        pkgs.rust-bin.stable."1.97.1".default.override {
          extensions = [
            "llvm-tools-preview"
            "rust-src"
            "rust-analyzer"
          ];
        };
      rustPlatformFor =
        pkgs:
        let
          toolchain = rustToolchainFor pkgs;
        in
        pkgs.makeRustPlatform {
          cargo = toolchain;
          rustc = toolchain;
        };
    in
    {
      packages = forAllSystems (
        system:
        let
          pkgs = pkgsFor system;
          pnpm = pkgs.pnpm_11.override { nodejs-slim = pkgs.nodejs-slim_22; };
          rustPlatform = rustPlatformFor pkgs;
          frontend = pkgs.stdenvNoCC.mkDerivation (finalAttrs: {
            pname = "marginalis-web-ui";
            inherit version;
            src = ./frontend;
            pnpmDeps = pkgs.fetchPnpmDeps {
              inherit (finalAttrs)
                pname
                version
                src
                ;
              inherit pnpm;
              fetcherVersion = 4;
              hash = "sha256-J9vYJ37K96VBLcbN5tbc4tQhM/f8YQcMU/dRKQkzdH0=";
            };
            nativeBuildInputs = [
              pkgs.nodejs_22
              pkgs.pnpmConfigHook
              pnpm
            ];
            buildPhase = ''
              runHook preBuild
              pnpm build
              runHook postBuild
            '';
            installPhase = ''
              runHook preInstall
              mkdir -p $out
              cp -r dist $out/dist
              runHook postInstall
            '';
          });
        in
        {
          inherit frontend;
          default = rustPlatform.buildRustPackage {
            pname = "marginalis";
            inherit version;
            src = pkgs.lib.fileset.toSource {
              root = ./.;
              fileset = pkgs.lib.fileset.unions [
                ./Cargo.toml
                ./Cargo.lock
                ./crates
                ./docs/openapi.json
                ./frontend
              ];
            };
            cargoLock = {
              lockFile = ./Cargo.lock;
              outputHashes = {
                "adocweave-0.19.0" = "sha256-DTJ0B6f5WHKt5bIvM8KghbIj5uNrf8HIL7iPoxILkrI=";
              };
            };
            cargoBuildFlags = [
              "--package"
              "marginalis-service"
              "--bin"
              "marginalis-service"
            ];
            preBuild = ''
              mkdir -p frontend
              cp -r ${frontend}/dist frontend/dist
            '';
            doCheck = false;
            installPhase = ''
              install -Dm755 target/${pkgs.stdenv.hostPlatform.rust.cargoShortTarget}/release/marginalis-service $out/bin/marginalis
              install -Dm644 docs/openapi.json $out/share/marginalis/openapi.json
            '';
          };
        }
      );

      nixosModules.default = import ./nix/module.nix self;

      checks = forAllSystems (
        system:
        let
          pkgs = pkgsFor system;
          marginalisV050Schema = pkgs.fetchurl {
            url = "https://raw.githubusercontent.com/KeishiS/marginalis/v0.5.0/crates/marginalis-sqlite/src/schema.sql";
            hash = "sha256-U8R8xzBYkohX+zKr3TtLlmvTPMhif+EBylhF+2L9u64=";
          };
          marginalisV090Source = builtins.fetchTarball {
            url = "https://github.com/KeishiS/marginalis/archive/9a286ebbb0a86065138cc658e46628175ba876e2.tar.gz";
            sha256 = "sha256-ljexEdr0oaF/u5IiMpWj7W98tdyDthmDeotxfBhj2CM=";
          };
          marginalisV090AdocweaveConformanceCases = pkgs.fetchurl {
            url = "https://raw.githubusercontent.com/KeishiS/adocweave/778e9da4548f03ea8434677d50c819d7ce665809/fixtures/conformance/cases.json";
            hash = "sha256-OxHK8NobfmNN9pRj7B3qP94s1b2E26l5y5EQdMQq6aY=";
          };
          marginalisV090 = (rustPlatformFor pkgs).buildRustPackage {
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
          kanidmDiscoveryCerts =
            pkgs.runCommand "marginalis-kanidm-discovery-certs"
              {
                nativeBuildInputs = [ pkgs.openssl ];
              }
              ''
                # Nix storeに保存されたfixtureを日をまたいで再利用しても失効しないようにする。
                openssl req -x509 -newkey rsa:2048 -nodes -days 36500 \
                  -subj '/CN=Marginalis Kanidm Test CA' \
                  -addext 'basicConstraints=critical,CA:TRUE' \
                  -addext 'keyUsage=critical,keyCertSign' \
                  -keyout ca-key.pem -out ca-cert.pem
                openssl req -newkey rsa:2048 -nodes \
                  -subj '/CN=id.example.test' \
                  -addext 'subjectAltName=DNS:id.example.test' \
                  -keyout $out-key.pem -out request.pem
                openssl x509 -req -in request.pem -CA ca-cert.pem -CAkey ca-key.pem \
                  -CAcreateserial -days 36500 -out $out-cert.pem \
                  -extfile <(printf 'basicConstraints=critical,CA:FALSE\nkeyUsage=critical,digitalSignature,keyEncipherment\nsubjectAltName=DNS:id.example.test')
                mkdir -p $out
                mv $out-key.pem $out/id-key.pem
                mv $out-cert.pem $out/id-cert.pem
                openssl req -newkey rsa:2048 -nodes \
                  -subj '/CN=marginalis.example.test' \
                  -addext 'subjectAltName=DNS:marginalis.example.test' \
                  -keyout $out/app-key.pem -out app-request.pem
                openssl x509 -req -in app-request.pem -CA ca-cert.pem -CAkey ca-key.pem \
                  -CAserial ca-cert.srl -days 36500 -out $out/app-cert.pem \
                  -extfile <(printf 'basicConstraints=critical,CA:FALSE\nkeyUsage=critical,digitalSignature,keyEncipherment\nsubjectAltName=DNS:marginalis.example.test')
                mv ca-cert.pem $out/ca.pem
              '';
          mcpAuthorizationFixture =
            pkgs.runCommand "marginalis-mcp-authorization-fixture"
              {
                nativeBuildInputs = [
                  pkgs.coreutils
                  pkgs.jq
                  pkgs.openssl
                  pkgs.xxd
                ];
              }
              ''
                mkdir -p $out
                # Nix storeに保存されたfixtureを日をまたいで再利用しても失効しないようにする。
                openssl req -x509 -newkey rsa:2048 -nodes -days 36500 \
                  -subj '/CN=Marginalis MCP Test CA' \
                  -addext 'basicConstraints=critical,CA:TRUE' \
                  -addext 'keyUsage=critical,keyCertSign' \
                  -keyout ca-key.pem -out $out/ca.pem
                openssl req -newkey rsa:2048 -nodes \
                  -subj '/CN=auth.example.test' \
                  -addext 'subjectAltName=DNS:auth.example.test' \
                  -keyout $out/server-key.pem -out server-request.pem
                openssl x509 -req -in server-request.pem -CA $out/ca.pem -CAkey ca-key.pem \
                  -CAcreateserial -days 36500 -out $out/server-cert.pem \
                  -extfile <(printf 'basicConstraints=critical,CA:FALSE\nkeyUsage=critical,digitalSignature,keyEncipherment\nsubjectAltName=DNS:auth.example.test')
                openssl genpkey -algorithm RSA -pkeyopt rsa_keygen_bits:2048 \
                  -out $out/signing-key.pem
                modulus="$(
                  openssl rsa -in $out/signing-key.pem -modulus -noout \
                    | cut -d= -f2 | xxd -r -p | base64 -w0 \
                    | tr '+/' '-_' | tr -d '='
                )"
                jq -n --arg modulus "$modulus" \
                  '{keys:[{kty:"RSA",use:"sig",kid:"test-key",alg:"RS256",n:$modulus,e:"AQAB"}]}' \
                  > $out/jwks.json
                chmod 0400 $out/server-key.pem $out/signing-key.pem
              '';
        in
        pkgs.lib.optionalAttrs pkgs.stdenv.isLinux {
          schema9-to-schema11-archive-migration =
            pkgs.runCommand "marginalis-schema9-to-schema11-archive-migration"
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

                sqlite3 schema9.sqlite <<'SQL'
                INSERT INTO notes
                  (note_id, creator_issuer, creator_subject, title, source, tags_json,
                   created_at_ms, updated_at_ms, revision, deleted_at_ms)
                VALUES
                  ('019f0000-0000-7000-8000-000000000091',
                   'https://id.example.test', 'migration-owner', '移行元',
                   '= 移行元
                :tags: 移行, 検証

                xref:note:019f0000-0000-7000-8000-000000000092[移行先]',
                   '["検証","移行"]', 1000, 4000, 4, NULL),
                  ('019f0000-0000-7000-8000-000000000092',
                   'https://id.example.test', 'migration-owner', '移行先',
                   '= 移行先

                削除済みの本文', '[]', 2000, 6000, 2, 6000);
                INSERT INTO note_references (source_note_id, target_note_id)
                VALUES ('019f0000-0000-7000-8000-000000000091',
                        '019f0000-0000-7000-8000-000000000092');
                INSERT INTO note_acl (note_id, issuer, subject, permission)
                VALUES
                  ('019f0000-0000-7000-8000-000000000091',
                   'https://id.example.test', 'migration-reader', 'read'),
                  ('019f0000-0000-7000-8000-000000000091',
                   'https://id.example.test', 'migration-editor', 'edit');
                SQL
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
                  migrate-archive --input "$PWD/schema9.json" --output "$PWD/schema11.json"
                cmp schema9.json schema9-original.json
                jq -e '
                  .format == "marginalis-archive-9"
                  and .adocweave_package_version == "0.19.0"
                  and .note_profile_version == 4
                  and (.notes | length) == 2
                  and (.note_acl | length) == 2
                ' schema11.json

                export MARGINALIS_DATABASE_URL="sqlite:$PWD/schema11.sqlite"
                ${self.packages.${system}.default}/bin/marginalis \
                  import-archive --input "$PWD/schema11.json"
                test "$(sqlite3 schema11.sqlite \
                  'SELECT MAX(version) FROM schema_migrations')" = 11
                sqlite3 -json schema11.sqlite \
                  'SELECT note_id, creator_issuer, creator_subject, title, source,
                          tags_json, created_at_ms, updated_at_ms, revision, deleted_at_ms
                   FROM notes ORDER BY note_id' > schema11-notes.json
                sqlite3 -json schema11.sqlite \
                  'SELECT source_note_id, target_note_id
                   FROM note_references ORDER BY source_note_id, target_note_id' \
                  > schema11-references.json
                sqlite3 -json schema11.sqlite \
                  'SELECT note_id, issuer, subject, permission
                   FROM note_acl ORDER BY note_id, issuer, subject' > schema11-acl.json
                diff -u schema9-notes.json schema11-notes.json
                cmp schema9-references.json schema11-references.json
                cmp schema9-acl.json schema11-acl.json
                test "$(sqlite3 schema11.sqlite \
                  "SELECT COUNT(*) FROM sqlite_schema
                   WHERE type = 'table' AND name IN
                     ('mcp_clients', 'mcp_authorization_codes',
                      'mcp_access_tokens', 'mcp_refresh_tokens')")" = 0

                ${self.packages.${system}.default}/bin/marginalis \
                  export-archive --output "$PWD/schema11-roundtrip.json"
                cmp schema11.json schema11-roundtrip.json
                ${self.packages.${system}.default}/bin/marginalis \
                  verify-restore --input "$PWD/schema11-roundtrip.json"
                touch $out
              '';

          nixos-module =
            let
              disabled = nixpkgs.lib.nixosSystem {
                inherit system;
                modules = [
                  self.nixosModules.default
                  { system.stateVersion = "25.11"; }
                ];
              };
              evaluated = nixpkgs.lib.nixosSystem {
                inherit system;
                modules = [
                  self.nixosModules.default
                  {
                    system.stateVersion = "25.11";
                    services.marginalis = {
                      enable = true;
                      openFirewall = true;
                      baseUrl = "https://marginalis.example.test";
                      oidc = {
                        issuerUrl = "https://id.example.test";
                        clientId = "marginalis";
                        clientSecretFile = "/run/secrets/marginalis-oidc-client-secret";
                      };
                      mcp = {
                        enable = true;
                        authorization = {
                          issuer = "https://evaluation.jp.auth0.com/";
                          upstreamIssuerClaim = "https://marginalis.example.test/claims/upstream-issuer";
                          upstreamSubjectClaim = "https://marginalis.example.test/claims/upstream-subject";
                          groupsClaim = "https://marginalis.example.test/claims/groups";
                        };
                      };
                    };
                  }
                ];
              };
              partialAuthorization = nixpkgs.lib.nixosSystem {
                inherit system;
                modules = [
                  self.nixosModules.default
                  {
                    system.stateVersion = "25.11";
                    services.marginalis = {
                      enable = true;
                      baseUrl = "https://marginalis.example.test";
                      oidc = {
                        issuerUrl = "https://id.example.test";
                        clientId = "marginalis";
                        clientSecretFile = "/run/secrets/marginalis-oidc-client-secret";
                      };
                      mcp.authorization.issuer = "https://evaluation.jp.auth0.com/";
                    };
                  }
                ];
              };
              mcpWithoutAuthorization = nixpkgs.lib.nixosSystem {
                inherit system;
                modules = [
                  self.nixosModules.default
                  {
                    system.stateVersion = "25.11";
                    services.marginalis = {
                      enable = true;
                      baseUrl = "https://marginalis.example.test";
                      oidc = {
                        issuerUrl = "https://id.example.test";
                        clientId = "marginalis";
                        clientSecretFile = "/run/secrets/marginalis-oidc-client-secret";
                      };
                      mcp.enable = true;
                    };
                  }
                ];
              };
              authorizationWithoutMcp = nixpkgs.lib.nixosSystem {
                inherit system;
                modules = [
                  self.nixosModules.default
                  {
                    system.stateVersion = "25.11";
                    services.marginalis = {
                      enable = true;
                      baseUrl = "https://marginalis.example.test";
                      oidc = {
                        issuerUrl = "https://id.example.test";
                        clientId = "marginalis";
                        clientSecretFile = "/run/secrets/marginalis-oidc-client-secret";
                      };
                      mcp.authorization = {
                        issuer = "https://evaluation.jp.auth0.com/";
                        upstreamIssuerClaim = "https://marginalis.example.test/claims/upstream-issuer";
                        upstreamSubjectClaim = "https://marginalis.example.test/claims/upstream-subject";
                        groupsClaim = "https://marginalis.example.test/claims/groups";
                      };
                    };
                  }
                ];
              };
            in
            assert evaluated.config.networking.firewall.allowedTCPPorts == [ 3000 ];
            assert
              evaluated.config.systemd.services.marginalis.environment.MARGINALIS_MCP_AUTHORIZATION_ISSUER
              == "https://evaluation.jp.auth0.com/";
            assert
              evaluated.config.systemd.services.marginalis-diagnose.environment.MARGINALIS_MCP_GROUPS_CLAIM
              == "https://marginalis.example.test/claims/groups";
            assert builtins.any (
              assertion:
              !assertion.assertion
              &&
                assertion.message == "services.marginalis.mcp.authorization options must be all set or all unset."
            ) partialAuthorization.config.assertions;
            assert builtins.any (
              assertion:
              !assertion.assertion
              &&
                assertion.message
                == "services.marginalis.mcp.authorization options must be set when MCP is enabled."
            ) mcpWithoutAuthorization.config.assertions;
            assert builtins.any (
              assertion:
              !assertion.assertion
              && assertion.message == "services.marginalis.mcp.enable must be true when authorization is set."
            ) authorizationWithoutMcp.config.assertions;
            assert evaluated.config.systemd.services.marginalis-diagnose.serviceConfig.ProtectKernelTunables;
            assert builtins.elem evaluated.config.services.marginalis.package
              evaluated.config.environment.systemPackages;
            assert
              !builtins.elem disabled.config.services.marginalis.package disabled.config.environment.systemPackages;
            assert
              evaluated.config.systemd.services.marginalis-purge-expired.serviceConfig.SystemCallFilter == [
                "@system-service"
                "~@privileged"
              ];
            pkgs.writeText "marginalis-nixos-module-evaluation" evaluated.config.systemd.services.marginalis.serviceConfig.ExecStart;

          nixos-module-vm =
            let
              probeServer = pkgs.writeShellApplication {
                name = "marginalis";
                text = ''
                  test "$PWD" = "/var/lib/marginalis"
                  test "$RUST_LOG" = "info,marginalis_auth_oidc=info"
                  if [ "''${1-}" = "backup" ] && [ "''${2-}" = "--directory" ]; then
                    test "$3" = "/var/lib/marginalis-backups/test"
                    touch "$3/backup-created"
                    exit 0
                  fi
                  if [ "''${1-}" = "prune-backups" ]; then
                    test "$2" = "--directory"
                    test "$3" = "/var/lib/marginalis-backups/test"
                    test "$4" = "--keep"
                    test "$5" = "30"
                    touch "$3/prune-completed"
                    exit 0
                  fi
                  if [ "''${1-}" = "verify-latest-backup" ]; then
                    test "$2" = "--directory"
                    test "$3" = "/var/lib/marginalis-backups/test"
                    exit 0
                  fi
                  test -s "$OIDC_CLIENT_SECRET_FILE"
                  touch "$PWD/service-started"
                  exec sleep infinity
                '';
              };
            in
            pkgs.testers.nixosTest {
              name = "marginalis-nixos-module";
              nodes.machine = {
                imports = [ self.nixosModules.default ];
                system.stateVersion = "25.11";

                environment.etc."marginalis-test/oidc-client-secret".text = "test-only-secret";
                services.marginalis = {
                  enable = true;
                  package = probeServer;
                  baseUrl = "https://marginalis.example.test";
                  backupDirectory = "/var/lib/marginalis-backups/test";
                  restoreCheck.enable = true;
                  oidc = {
                    issuerUrl = "https://id.example.test";
                    clientId = "marginalis";
                    clientSecretFile = "/etc/marginalis-test/oidc-client-secret";
                  };
                };
              };

              testScript = ''
                machine.wait_for_unit("marginalis.service")
                machine.succeed(
                  "test $(readlink -f $(command -v marginalis)) = ${probeServer}/bin/marginalis"
                )
                machine.succeed("test -f /var/lib/marginalis/service-started")
                machine.succeed("systemctl restart marginalis.service")
                machine.wait_for_unit("marginalis.service")
                machine.succeed("test -f /var/lib/marginalis/service-started")
                machine.succeed("systemctl start marginalis-backup.service")
                machine.succeed("test -f /var/lib/marginalis-backups/test/backup-created")
                machine.succeed("test -f /var/lib/marginalis-backups/test/prune-completed")
                machine.succeed("systemctl start marginalis-restore-check.service")
                machine.succeed("systemctl is-enabled marginalis-backup.timer")
                machine.succeed("systemctl is-enabled marginalis-restore-check.timer")
                machine.succeed("systemctl is-active marginalis.service")
              '';
            };
          nixos-module-runtime-vm = pkgs.testers.nixosTest {
            name = "marginalis-nixos-module-runtime";
            nodes.machine = {
              imports = [ self.nixosModules.default ];
              system.stateVersion = "25.11";
              environment.systemPackages = [
                pkgs.curl
                pkgs.jq
                pkgs.sqlite
              ];
              environment.etc."marginalis-test/oidc-client-secret".text = "test-only-secret";
              services.marginalis = {
                enable = true;
                baseUrl = "https://marginalis.example.test";
                backupDirectory = "/var/lib/marginalis-backups/test";
                restoreCheck.enable = true;
                oidc = {
                  # networkに依存せず、OIDC未到達時にもlivenessを維持してloginをfail closedにする経路を検証する。
                  issuerUrl = "https://127.0.0.1:1";
                  clientId = "marginalis";
                  clientSecretFile = "/etc/marginalis-test/oidc-client-secret";
                };
              };
            };

            testScript = ''
              machine.wait_for_unit("marginalis.service")
              machine.succeed(
                "test \"$(/run/current-system/sw/bin/marginalis --version)\" = 'marginalis 0.16.1'"
              )
              machine.wait_until_succeeds(
                  "curl -fsS http://127.0.0.1:3000/api/v3/health | jq -e '.status == \"ok\" and .api_version == \"v3\"'"
              )
              machine.wait_until_succeeds(
                "systemctl status marginalis.service --no-pager --full | "
                + "grep -F 'http.request.completed' | grep -F 'outcome=' | grep -Fq success"
              )
              machine.succeed(
                "test $(curl --max-time 15 -sS -o /dev/null -w '%{http_code}' http://127.0.0.1:3000/auth/oidc/login) = 503"
              )
              machine.wait_until_succeeds(
                "systemctl status marginalis.service --no-pager --full | "
                + "grep -F 'http.request.completed' | grep -F 'status=503' | "
                + "grep -F 'outcome=' | grep -Fq failure"
              )
              machine.succeed(
                  "curl -fsS http://127.0.0.1:3000/api/v3/openapi.json | jq -e '.openapi == \"3.1.0\"'"
              )
              machine.succeed("sqlite3 /var/lib/marginalis/marginalis.sqlite 'SELECT 1 FROM notes'")
              machine.succeed(
                "sqlite3 /var/lib/marginalis/marginalis.sqlite \"INSERT INTO notes "
                + "(note_id,creator_issuer,creator_subject,title,source,tags_json,created_at_ms,updated_at_ms,revision,deleted_at_ms) VALUES "
                + "('019f0000-0000-7000-8000-000000000001','https://id.example.test','stale','stale','= stale','[]',0,0,1,0),"
                + "('019f0000-0000-7000-8000-000000000002','https://id.example.test','recent','recent','= recent','[]',0,4102444800000,1,4102444800000);\""
              )
              machine.succeed("systemctl start marginalis-purge-expired.service")
              machine.succeed(
                "test $(sqlite3 /var/lib/marginalis/marginalis.sqlite "
                + "\"SELECT COUNT(*) FROM notes WHERE note_id = '019f0000-0000-7000-8000-000000000001'\") -eq 0"
              )
              machine.succeed(
                "test $(sqlite3 /var/lib/marginalis/marginalis.sqlite "
                + "\"SELECT COUNT(*) FROM notes WHERE note_id = '019f0000-0000-7000-8000-000000000002'\") -eq 1"
              )
              machine.succeed("systemctl is-enabled marginalis-purge-expired.timer")
              machine.succeed("systemctl is-enabled marginalis-backup.timer")
              machine.succeed("systemctl is-enabled marginalis-restore-check.timer")
              machine.succeed("systemctl start marginalis-diagnose.service")
              machine.succeed(
                "journalctl -u marginalis-diagnose.service -o cat | "
                + "grep '^{\"status\":\"ok\"' | tail -1 | jq -e "
                + "'.database.available and .database.schema.ok and .database.integrity.ok and .database.foreign_keys.ok'"
              )
              machine.succeed(
                "journalctl -u marginalis.service -o cat | grep -Fq 'oidc.discovery.failed'"
              )
              machine.succeed("systemctl start marginalis-backup.service")
              machine.succeed("systemctl is-active marginalis.service")
              machine.succeed(
                "test $(find /var/lib/marginalis-backups/test -mindepth 1 -maxdepth 1 -type d | wc -l) -eq 1"
              )
              machine.succeed(
                "backup=$(find /var/lib/marginalis-backups/test -mindepth 1 -maxdepth 1 -type d); "
                + "test -f \"$backup/COMPLETE\"; "
                + "test -f \"$backup/marginalis-archive.json\"; "
                + "jq -e '.format == \"marginalis-archive-9\" "
                + "and .adocweave_package_version == \"0.19.0\" "
                + "and .note_profile_version == 4 and (.notes | length == 1)' "
                + "\"$backup/marginalis-archive.json\"; "
                + "test $(stat -c %a \"$backup\") = 700; "
                + "test $(stat -c %a \"$backup/COMPLETE\") = 600; "
                + "test $(stat -c %a \"$backup/marginalis-archive.json\") = 600"
              )
              machine.succeed("systemctl start marginalis-restore-check.service")
              machine.succeed(
                "backup=$(find /var/lib/marginalis-backups/test -mindepth 1 -maxdepth 1 -type d); "
                + "MARGINALIS_DATABASE_URL=sqlite:/tmp/restored.sqlite "
                + "${self.packages.${system}.default}/bin/marginalis import-archive "
                + "--input \"$backup/marginalis-archive.json\"; "
                + "test $(sqlite3 /tmp/restored.sqlite 'SELECT COUNT(*) FROM notes') -eq 1; "
                + "test $(sqlite3 /tmp/restored.sqlite "
                + "\"SELECT COUNT(*) FROM notes WHERE deleted_at_ms IS NOT NULL AND revision = 1 "
                + "AND creator_issuer = 'https://id.example.test' AND creator_subject = 'recent'\") -eq 1; "
                + "test $(sqlite3 /tmp/restored.sqlite "
                + "\"SELECT COUNT(*) FROM sqlite_schema WHERE type = 'table' AND name = 'note_acl'\") -eq 1"
              )
              machine.fail(
                "backup=$(find /var/lib/marginalis-backups/test -mindepth 1 -maxdepth 1 -type d); "
                + "MARGINALIS_DATABASE_URL=sqlite:/tmp/restored.sqlite "
                + "${self.packages.${system}.default}/bin/marginalis import-archive "
                + "--input \"$backup/marginalis-archive.json\""
              )
              machine.succeed("mount -t tmpfs -o size=64K tmpfs /var/lib/marginalis-backups/test")
              machine.succeed(
                "dd if=/dev/zero of=/var/lib/marginalis-backups/test/fill bs=4096 status=none || true"
              )
              machine.fail("systemctl start marginalis-backup.service")
              machine.succeed(
                "journalctl -u marginalis-backup.service -o cat | "
                + "grep -Fq 'maintenance.backup.failed'"
              )
              machine.succeed(
                "! find /var/lib/marginalis-backups/test -name COMPLETE -print -quit | grep -q ."
              )
              machine.succeed("umount /var/lib/marginalis-backups/test")
              machine.succeed("systemctl stop marginalis.service")
              machine.succeed("cp /var/lib/marginalis/marginalis.sqlite /tmp/corrupt.sqlite")
              machine.succeed(
                "printf 'not-a-sqlite-database' | "
                + "dd of=/tmp/corrupt.sqlite bs=1 seek=0 conv=notrunc status=none"
              )
              machine.fail(
                "MARGINALIS_DATABASE_URL=sqlite:/tmp/corrupt.sqlite "
                + "${self.packages.${system}.default}/bin/marginalis diagnose > /tmp/corrupt-report.json"
              )
              machine.succeed(
                "jq -e '.status == \"failed\" and "
                + "(.database.available == false or .database.integrity.ok == false)' "
                + "/tmp/corrupt-report.json"
              )
              machine.succeed("chmod 0400 /var/lib/marginalis/marginalis.sqlite")
              machine.fail("systemctl start marginalis-purge-expired.service")
              machine.succeed(
                "journalctl -u marginalis-purge-expired.service -o cat | "
                + "grep -Fq 'maintenance.purge.failed'"
              )
              machine.succeed(
                "chown marginalis:marginalis /var/lib/marginalis/marginalis.sqlite* && "
                + "chmod 0600 /var/lib/marginalis/marginalis.sqlite*"
              )
              machine.execute("systemctl start marginalis.service")
              machine.wait_until_succeeds(
                "curl -fsS http://127.0.0.1:3000/api/v3/health | jq -e '.status == \"ok\"'"
              )
              machine.succeed("systemctl stop marginalis.service")
              machine.succeed(
                "runuser -u marginalis -- sqlite3 /var/lib/marginalis/marginalis.sqlite "
                + "'PRAGMA journal_mode=DELETE; UPDATE schema_migrations SET version = 1'"
              )
              machine.fail("systemctl start marginalis-diagnose.service")
              machine.succeed(
                "journalctl -u marginalis-diagnose.service -o cat | "
                + "grep '^{\"status\":\"failed\"' | tail -1 | jq -e "
                + "'.database.schema.ok == false and .database.schema.actual == 1 and .database.schema.expected == 11'"
              )
              machine.succeed(
                "runuser -u marginalis -- sqlite3 /var/lib/marginalis/marginalis.sqlite "
                + "'UPDATE schema_migrations SET version = 10; PRAGMA journal_mode=WAL'"
              )
              machine.succeed("rm -f /var/lib/marginalis/marginalis.sqlite*")
              machine.succeed(
                "sqlite3 /var/lib/marginalis/marginalis.sqlite "
                + "\"CREATE TABLE schema_migrations (version INTEGER PRIMARY KEY NOT NULL); "
                + "INSERT INTO schema_migrations (version) VALUES (4);\""
              )
              machine.succeed(
                "sqlite3 /var/lib/marginalis/marginalis.sqlite < ${marginalisV050Schema}"
              )
              machine.succeed(
                "sqlite3 /var/lib/marginalis/marginalis.sqlite "
                + "\"UPDATE schema_migrations SET version = 5;\""
              )
              machine.succeed(
                "sqlite3 /var/lib/marginalis/marginalis.sqlite \"INSERT INTO notes "
                + "(note_id,creator_issuer,creator_subject,title,body,tags_json,created_at_ms,updated_at_ms,revision,deleted_at_ms) VALUES "
                + "('019f0000-0000-7000-8000-000000000050','https://id.example.test','v0.5-user',"
                + "'v0.5 note','kept across update','[\\\"upgrade\\\"]',1,2,3,NULL);\""
              )
              machine.succeed(
                "chown marginalis:marginalis /var/lib/marginalis/marginalis.sqlite && "
                + "chmod 0600 /var/lib/marginalis/marginalis.sqlite"
              )
              machine.execute("systemctl start marginalis.service")
              machine.wait_until_succeeds(
                "timeout 5s journalctl --no-pager -u marginalis.service -o cat | "
                + "grep -F 'unsupported database schema version 5; expected 11'"
              )
              machine.succeed("systemctl stop marginalis.service")
              machine.succeed(
                "test ! -e /var/lib/marginalis/marginalis.sqlite-wal && "
                + "test ! -e /var/lib/marginalis/marginalis.sqlite-shm && "
                + "sha256sum /var/lib/marginalis/marginalis.sqlite > /tmp/schema5.sha256 && "
                + "stat -c '%a:%U:%G' /var/lib/marginalis/marginalis.sqlite > /tmp/schema5.metadata"
              )
              for _ in range(5):
                  machine.succeed("systemctl reset-failed marginalis-diagnose.service")
                  machine.fail("systemctl start marginalis-diagnose.service")
                  machine.succeed(
                    "journalctl -u marginalis-diagnose.service -o cat | "
                    + "grep '^{\"status\":\"failed\"' | tail -1 | jq -e "
                    + "'.database.schema.ok == false "
                    + "and .database.schema.actual == 5 "
                    + "and .database.schema.expected == 11 "
                    + "and .database.integrity.ok "
                    + "and .database.integrity.actual == \"ok\" "
                    + "and .database.foreign_keys.ok "
                    + "and .database.foreign_keys.actual == 0 "
                    + "and ((.database.failures // []) | length) == 0 "
                    + "and (.database.error? == null)'"
                  )
              machine.succeed(
                "sha256sum --check /tmp/schema5.sha256 && "
                + "test \"$(stat -c '%a:%U:%G' /var/lib/marginalis/marginalis.sqlite)\" "
                + "= \"$(cat /tmp/schema5.metadata)\" && "
                + "test ! -e /var/lib/marginalis/marginalis.sqlite-wal && "
                + "test ! -e /var/lib/marginalis/marginalis.sqlite-shm"
              )
              machine.succeed(
                "journalctl -u marginalis-diagnose.service -o cat | "
                + "grep -Fq 'maintenance.diagnostics.failed'"
              )
            '';
          };

          mcp-authorization-vm = pkgs.testers.nixosTest {
            name = "marginalis-mcp-authorization";
            nodes.authorization = {
              networking.firewall.allowedTCPPorts = [ 443 ];
              environment.systemPackages = [ pkgs.python3 ];
              systemd.services.mcp-authorization-fixture = {
                wantedBy = [ "multi-user.target" ];
                after = [ "network.target" ];
                serviceConfig.ExecStart = "${pkgs.python3}/bin/python3 ${pkgs.writeText "mcp-authorization-fixture.py" ''
                  import http.server
                  import json
                  import ssl

                  fixture = "${mcpAuthorizationFixture}"
                  metadata = {
                      "issuer": "https://auth.example.test/",
                      "jwks_uri": "https://auth.example.test/jwks.json",
                  }

                  class Handler(http.server.BaseHTTPRequestHandler):
                      def do_GET(self):
                          if self.path == "/.well-known/oauth-authorization-server":
                              body = json.dumps(metadata).encode()
                          elif self.path == "/jwks.json":
                              with open(f"{fixture}/jwks.json", "rb") as source:
                                  body = source.read()
                          else:
                              self.send_error(404)
                              return
                          self.send_response(200)
                          self.send_header("Content-Type", "application/json")
                          self.send_header("Content-Length", str(len(body)))
                          self.end_headers()
                          self.wfile.write(body)

                      def log_message(self, format, *args):
                          pass

                  server = http.server.ThreadingHTTPServer(("0.0.0.0", 443), Handler)
                  context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
                  context.load_cert_chain(
                      f"{fixture}/server-cert.pem", f"{fixture}/server-key.pem"
                  )
                  server.socket = context.wrap_socket(server.socket, server_side=True)
                  server.serve_forever()
                ''}";
              };
            };
            nodes.app = {
              imports = [ self.nixosModules.default ];
              system.stateVersion = "25.11";
              networking.hosts."192.168.1.2" = [ "auth.example.test" ];
              security.pki.certificateFiles = [ "${mcpAuthorizationFixture}/ca.pem" ];
              environment.etc."marginalis-test/oidc-client-secret".text = "test-only-secret";
              environment.systemPackages = [
                pkgs.coreutils
                pkgs.curl
                pkgs.jq
                pkgs.openssl
              ];
              services.marginalis = {
                enable = true;
                baseUrl = "https://marginalis.example.test";
                oidc = {
                  issuerUrl = "https://id.example.test";
                  clientId = "marginalis";
                  clientSecretFile = "/etc/marginalis-test/oidc-client-secret";
                };
                mcp = {
                  enable = true;
                  authorization = {
                    issuer = "https://auth.example.test/";
                    upstreamIssuerClaim = "https://marginalis.example.test/claims/upstream-issuer";
                    upstreamSubjectClaim = "https://marginalis.example.test/claims/upstream-subject";
                    groupsClaim = "https://marginalis.example.test/claims/groups";
                  };
                };
              };
            };

            testScript = ''
              authorization.start()
              authorization.wait_for_unit("mcp-authorization-fixture.service")
              app.start()
              app.wait_for_unit("marginalis.service")
              app.wait_until_succeeds(
                "curl -fsS http://127.0.0.1:3000/api/v3/health | jq -e '.status == \"ok\"'"
              )
              app.succeed(
                "curl -fsS http://127.0.0.1:3000/.well-known/oauth-protected-resource/mcp "
                + "| jq -e '.resource == \"https://marginalis.example.test/mcp\" "
                + "and .authorization_servers == [\"https://auth.example.test/\"]'"
              )
              app.succeed(
                "header=$(printf '%s' '{\"alg\":\"RS256\",\"typ\":\"JWT\",\"kid\":\"test-key\"}' "
                + "| base64 -w0 | tr '+/' '-_' | tr -d '='); "
                + "payload=$(printf '%s' '{\"iss\":\"https://auth.example.test/\","
                + "\"aud\":\"https://marginalis.example.test/mcp\",\"exp\":4102444800,"
                + "\"https://marginalis.example.test/claims/upstream-issuer\":\"https://id.example.test/\","
                + "\"https://marginalis.example.test/claims/upstream-subject\":\"test-user\","
                + "\"https://marginalis.example.test/claims/groups\":[\"server-users\"],"
                + "\"scope\":\"notes:read notes:write notes:delete\"}' "
                + "| base64 -w0 | tr '+/' '-_' | tr -d '='); "
                + "signature=$(printf '%s' \"$header.$payload\" "
                + "| openssl dgst -sha256 -sign ${mcpAuthorizationFixture}/signing-key.pem "
                + "| base64 -w0 | tr '+/' '-_' | tr -d '='); "
                + "token=\"$header.$payload.$signature\"; "
                + "curl -fsS -H \"Authorization: Bearer $token\" "
                + "-H 'Accept: application/json, text/event-stream' "
                + "-H 'Content-Type: application/json' "
                + "--data '{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/list\"}' "
                + "http://127.0.0.1:3000/mcp "
                + "| jq -e '.result.tools | length > 0'"
              )
              app.succeed(
                "journalctl -u marginalis.service -o cat "
                + "| grep -Fq 'mcp.authorization.discovery.completed'"
              )
              authorization.succeed("systemctl stop mcp-authorization-fixture.service")
              app.succeed("systemctl restart marginalis.service || true")
              app.wait_until_succeeds(
                "journalctl -u marginalis.service -o cat "
                + "| grep -Fq 'mcp.authorization.discovery.failed'"
              )
              app.succeed("systemctl stop marginalis.service")
            '';
          };

          # 実 Kanidm 1.10、private CA、nginx TLS、subpathを通して、Web用OIDC Discoveryと
          # browser login開始を確認する。対話loginとgroup変更は手動受入で扱う。
          kanidm-discovery-vm = pkgs.testers.nixosTest {
            name = "marginalis-kanidm-discovery";
            nodes.idp = {
              environment.systemPackages = [ pkgs.kanidm_1_10 ];
              services.kanidm = {
                package = pkgs.kanidmWithSecretProvisioning_1_10;
                server = {
                  enable = true;
                  settings = {
                    origin = "https://id.example.test:8443";
                    domain = "id.example.test";
                    bindaddress = "0.0.0.0:8443";
                    tls_chain = "${kanidmDiscoveryCerts}/id-cert.pem";
                    tls_key = "${kanidmDiscoveryCerts}/id-key.pem";
                  };
                };
                provision = {
                  enable = true;
                  instanceUrl = "https://localhost:8443";
                  acceptInvalidCerts = true;
                  idmAdminPasswordFile = pkgs.writeText "marginalis-test-idm-admin-password" "test-idm-admin-password";
                  groups.server-users = { };
                  systems.oauth2.marginalis = {
                    displayName = "Marginalis test client";
                    originUrl = "https://marginalis.example.test/marginalis/auth/oidc/callback";
                    originLanding = "https://marginalis.example.test/marginalis";
                    basicSecretFile = pkgs.writeText "marginalis-test-oidc-secret" "test-only-secret";
                    scopeMaps.server-users = [
                      "openid"
                      "profile"
                      "email"
                      "groups_name"
                    ];
                    claimMaps.groups = {
                      joinType = "array";
                      valuesByGroup.server-users = [ "server-users" ];
                    };
                  };
                };
              };
              networking.firewall.allowedTCPPorts = [ 8443 ];
            };
            nodes.app =
              { nodes, ... }:
              {
                imports = [ self.nixosModules.default ];
                system.stateVersion = "25.11";
                # NixOS test driverのeth0 DHCPアドレスは各VMで重複するため、隔離VLANの
                # IdPアドレスを使う。idpは二番目に起動するVMなので192.168.1.2となる。
                networking.hosts."192.168.1.2" = [ "id.example.test" ];
                networking.hosts."127.0.0.1" = [ "marginalis.example.test" ];
                security.pki.certificateFiles = [ "${kanidmDiscoveryCerts}/ca.pem" ];
                environment.etc."marginalis-test/oidc-client-secret".text = "test-only-secret";
                environment.systemPackages = [
                  pkgs.kanidm_1_10
                  pkgs.playwright-test
                  pkgs.playwright-driver.browsers
                  pkgs.ripgrep
                  pkgs.sqlite
                ];
                services.nginx = {
                  enable = true;
                  virtualHosts."marginalis.example.test" = {
                    forceSSL = true;
                    sslCertificate = "${kanidmDiscoveryCerts}/app-cert.pem";
                    sslCertificateKey = "${kanidmDiscoveryCerts}/app-key.pem";
                    locations."/marginalis/".proxyPass = "http://127.0.0.1:3000/";
                  };
                };
                services.marginalis = {
                  enable = true;
                  baseUrl = "https://marginalis.example.test/marginalis";
                  oidc = {
                    issuerUrl = "https://id.example.test:8443/oauth2/openid/marginalis";
                    clientId = "marginalis";
                    clientSecretFile = "/etc/marginalis-test/oidc-client-secret";
                    caCertificateFile = "${kanidmDiscoveryCerts}/ca.pem";
                  };
                };
              };
            testScript = ''
              idp.start()
              idp.wait_for_unit("kanidm.service")
              idp.wait_until_succeeds(
                "curl --insecure --resolve id.example.test:8443:127.0.0.1 -Lsf https://id.example.test:8443 | grep Kanidm"
              )
              idp.succeed(
                "KANIDM_PASSWORD=test-idm-admin-password "
                + "kanidm login --accept-invalid-certs -H https://localhost:8443 -D idm_admin"
              )
              idp.succeed(
                "kanidm group add-members --accept-invalid-certs "
                + "-H https://localhost:8443 -D idm_admin server-users idm_admin"
              )
              app.start()
              app.wait_for_unit("marginalis.service")
              app.wait_for_unit("nginx.service")
              app.wait_until_succeeds("curl -fsS http://127.0.0.1:3000/api/v3/health | grep -q '\"api_version\":\"v3\"'")
              app.wait_until_succeeds(
                "systemctl status marginalis.service --no-pager --full | "
                + "grep -F 'http.request.completed' | grep -F 'outcome=' | grep -Fq success"
              )
              app.succeed(
                "headers=$(mktemp); "
                + "curl --cacert ${kanidmDiscoveryCerts}/ca.pem -sS -D \"$headers\" -o /dev/null "
                + "'https://marginalis.example.test/marginalis/auth/oidc/login?next=%2Fmarginalis%2F'; "
                + "grep -qi '^set-cookie: marginalis_return_to=.*Path=/marginalis.*Secure' \"$headers\"; "
                + "grep -qi '^location: https://id.example.test:8443/' \"$headers\""
              )
              app.succeed(
                "test $(curl --cacert ${kanidmDiscoveryCerts}/ca.pem -sS -o /dev/null -w '%{http_code}' "
                + "'https://marginalis.example.test/marginalis/auth/oidc/callback?code=invalid&state=invalid') = 401"
              )
              app.wait_until_succeeds(
                "systemctl status marginalis.service --no-pager --full | "
                + "grep -F 'http.request.completed' | grep -F 'status=401' | "
                + "grep -F 'outcome=' | grep -Fq rejected"
              )
              app.succeed(
                "sqlite3 /var/lib/marginalis/marginalis.sqlite \""
                + "INSERT INTO web_sessions "
                + "(session_id_hash,csrf_token_hash,issuer,subject,issued_at_ms,last_seen_at_ms,idle_expires_at_ms,absolute_expires_at_ms) VALUES "
                + "(X'9257575af58c9bed123fb881f8ed8ddac43449f996542b47d3f8ebd74affc997',X'06f4c546d56505fc3365ad0af9315b19674c857a5aa0eb07a4b520a373d5bb80','https://id.example.test:8443/oauth2/openid/marginalis','reader-subject',1000000000000,1000000000000,4000000000000,4000000000000),"
                + "(X'2af479431a32c17ea66d6eec48a390ca8051630ffea1b2ae75f7deff228286c7',X'2ac2f8b7dbd2b4547e48f2c6d78535c68977644aa94ebacf1334bf1d4069c5cb','https://id.example.test:8443/oauth2/openid/marginalis','editor-subject',1000000000000,1000000000000,4000000000000,4000000000000),"
                + "(X'621388f1a111b5f664f87a25c012d3f1776eb8e53bd0bfe95b7524b536e27d64',X'c635ac885a14aa3b00a3d3fcfc7c158a0975139fdc42defcebb751da43c436a8','https://id.example.test:8443/oauth2/openid/marginalis','outsider-subject',1000000000000,1000000000000,4000000000000,4000000000000);\""
              )
              app.succeed(
                "mkdir -p /tmp/fixtures; "
                + "cp ${./tests/browser/fixtures/browser-diagnostics.js} /tmp/fixtures/browser-diagnostics.js; "
                + "cp ${./tests/browser/kanidm-login.spec.js} /tmp/kanidm-login.spec.js; "
                + "cp ${./tests/browser/webui-editing.spec.js} /tmp/webui-editing.spec.js; "
                + "cp ${./tests/browser/webui-acl.spec.js} /tmp/webui-acl.spec.js; "
                + "cd /tmp; "
                + "set +e; playwright test kanidm-login.spec.js webui-editing.spec.js webui-acl.spec.js --reporter=line --workers=1 "
                + ">/tmp/playwright-raw.log 2>&1; status=$?; set -e; "
                + "bash ${./.github/scripts/protocol-artifact.sh} sanitize "
                + "/tmp/playwright-raw.log /tmp/playwright.log; "
                + "bash ${./.github/scripts/protocol-artifact.sh} check /tmp/playwright.log; "
                + "cat /tmp/playwright.log; exit $status"
              )
              app.succeed("journalctl -u marginalis.service | grep -q 'Marginalis server listening'")
              app.succeed("! journalctl -u marginalis.service | grep -q 'OIDC discovery is unavailable'")
            '';
          };
        }
      );

      devShells = forAllSystems (
        system:
        let
          pkgs = pkgsFor system;
          pnpm = pkgs.pnpm_11.override { nodejs-slim = pkgs.nodejs-slim_22; };
          rustToolchain = rustToolchainFor pkgs;
        in
        {
          default = pkgs.mkShell {
            packages = with pkgs; [
              curl
              actionlint
              cargo-audit
              cargo-llvm-cov
              dejavu_fonts
              fontconfig
              rustToolchain
              cargo-make
              git
              gh
              jq
              lld
              nix
              nixfmt
              nodejs_22
              pnpm
              noto-fonts-cjk-sans
              playwright-driver.browsers
              playwright-test
              ripgrep
              sqlite
              wasm-bindgen-cli
            ];

            RUST_SRC_PATH = "${rustToolchain}/lib/rustlib/src/rust/library";
            FONTCONFIG_FILE = "${pkgs.fontconfig.out}/etc/fonts/fonts.conf";
            FONTCONFIG_PATH = "${pkgs.fontconfig.out}/etc/fonts";
          };
        }
      );

      formatter = forAllSystems (system: (pkgsFor system).nixfmt);
    };
}
