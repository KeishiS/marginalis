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
        "x86_64-darwin"
        "x86_64-linux"
      ];
      forAllSystems = nixpkgs.lib.genAttrs systems;
      pkgsFor =
        system:
        import nixpkgs {
          inherit system;
          overlays = [ rust-overlay.overlays.default ];
        };
      # AdocWeave v0.11.0 が要求する Rust 1.97.1 を確定的にピンする。
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
          rustPlatform = rustPlatformFor pkgs;
          # adocweave は通常ビルドでリポジトリ直下の conformance manifest を
          # include_str! するため、crate 単位の Cargo vendoring ではこのファイルが
          # 欠落する。依存と同じコミットのファイルを内容ハッシュ付きで補う。
          adocweaveConformanceCases = pkgs.fetchurl {
            url = "https://raw.githubusercontent.com/KeishiS/AdocWeave/778e9da4548f03ea8434677d50c819d7ce665809/fixtures/conformance/cases.json";
            hash = "sha256-OxHK8NobfmNN9pRj7B3qP94s1b2E26l5y5EQdMQq6aY=";
          };
          frontend = pkgs.buildNpmPackage {
            pname = "marginalis-web-ui";
            version = "0.6.0";
            src = ./frontend;
            npmDepsHash = "sha256-LFrGXjc7wsKkon1drJOwimQPg264RSEDnuEUgPR5NVw=";
            nodejs = pkgs.nodejs_22;
            installPhase = ''
              mkdir -p $out
              cp -r dist $out/dist
            '';
          };
        in
        {
          inherit frontend;
          default = rustPlatform.buildRustPackage {
            pname = "marginalis";
            version = "0.6.0";
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
              install -Dm444 ${adocweaveConformanceCases} ../fixtures/conformance/cases.json
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
            url = "https://raw.githubusercontent.com/KeishiS/Marginalis/v0.5.0/crates/marginalis-sqlite/src/schema.sql";
            hash = "sha256-U8R8xzBYkohX+zKr3TtLlmvTPMhif+EBylhF+2L9u64=";
          };
          kanidmDiscoveryCerts =
            pkgs.runCommand "marginalis-kanidm-discovery-certs"
              {
                nativeBuildInputs = [ pkgs.openssl ];
              }
              ''
                openssl req -x509 -newkey rsa:2048 -nodes -days 1 \
                  -subj '/CN=Marginalis Kanidm Test CA' \
                  -addext 'basicConstraints=critical,CA:TRUE' \
                  -addext 'keyUsage=critical,keyCertSign' \
                  -keyout ca-key.pem -out ca-cert.pem
                openssl req -newkey rsa:2048 -nodes \
                  -subj '/CN=id.example.test' \
                  -addext 'subjectAltName=DNS:id.example.test' \
                  -keyout $out-key.pem -out request.pem
                openssl x509 -req -in request.pem -CA ca-cert.pem -CAkey ca-key.pem \
                  -CAcreateserial -days 1 -out $out-cert.pem \
                  -extfile <(printf 'basicConstraints=critical,CA:FALSE\nkeyUsage=critical,digitalSignature,keyEncipherment\nsubjectAltName=DNS:id.example.test')
                mkdir -p $out
                mv $out-key.pem $out/id-key.pem
                mv $out-cert.pem $out/id-cert.pem
                openssl req -newkey rsa:2048 -nodes \
                  -subj '/CN=marginalis.example.test' \
                  -addext 'subjectAltName=DNS:marginalis.example.test' \
                  -keyout $out/app-key.pem -out app-request.pem
                openssl x509 -req -in app-request.pem -CA ca-cert.pem -CAkey ca-key.pem \
                  -CAserial ca-cert.srl -days 1 -out $out/app-cert.pem \
                  -extfile <(printf 'basicConstraints=critical,CA:FALSE\nkeyUsage=critical,digitalSignature,keyEncipherment\nsubjectAltName=DNS:marginalis.example.test')
                mv ca-cert.pem $out/ca.pem
              '';
        in
        pkgs.lib.optionalAttrs pkgs.stdenv.isLinux {
          nixos-module =
            let
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
                    };
                  }
                ];
              };
            in
            assert evaluated.config.networking.firewall.allowedTCPPorts == [ 3000 ];
            assert evaluated.config.systemd.services.marginalis-diagnose.serviceConfig.ProtectKernelTunables;
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
              machine.wait_until_succeeds(
                  "curl -fsS http://127.0.0.1:3000/api/v2/health | jq -e '.status == \"ok\" and .api_version == \"v2\"'"
              )
              machine.succeed(
                  "test $(curl --max-time 15 -sS -o /dev/null -w '%{http_code}' http://127.0.0.1:3000/auth/oidc/login) = 503"
              )
              machine.succeed(
                  "curl -fsS http://127.0.0.1:3000/api/v2/openapi.json | jq -e '.openapi == \"3.1.0\"'"
              )
              machine.succeed("sqlite3 /var/lib/marginalis/marginalis.sqlite 'SELECT 1 FROM notes'")
              machine.succeed(
                "sqlite3 /var/lib/marginalis/marginalis.sqlite \"INSERT INTO notes "
                + "(note_id,creator_issuer,creator_subject,title,body,tags_json,created_at_ms,updated_at_ms,revision,deleted_at_ms) VALUES "
                + "('019f0000-0000-7000-8000-000000000001','https://id.example.test','stale','stale','body','[]',0,0,1,0),"
                + "('019f0000-0000-7000-8000-000000000002','https://id.example.test','recent','recent','body','[]',0,4102444800000,1,4102444800000);\""
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
                + "jq -e '.format == \"marginalis-archive-4\" "
                + "and .adocweave_package_version == \"0.11.0\" "
                + "and .note_profile_version == 2 and (.notes | length == 1)' "
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
                + "\"SELECT COUNT(*) FROM sqlite_schema WHERE type = 'table' AND name = 'note_acl'\") -eq 0"
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
              machine.succeed("systemctl start marginalis.service")
              machine.wait_until_succeeds(
                "curl -fsS http://127.0.0.1:3000/api/v2/health | jq -e '.status == \"ok\"'"
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
                + "'.database.schema.ok == false and .database.schema.actual == 1 and .database.schema.expected == 4'"
              )
              machine.succeed(
                "runuser -u marginalis -- sqlite3 /var/lib/marginalis/marginalis.sqlite "
                + "'UPDATE schema_migrations SET version = 4; PRAGMA journal_mode=WAL'"
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
                "sqlite3 /var/lib/marginalis/marginalis.sqlite \"INSERT INTO notes "
                + "(note_id,creator_issuer,creator_subject,title,body,tags_json,created_at_ms,updated_at_ms,revision,deleted_at_ms) VALUES "
                + "('019f0000-0000-7000-8000-000000000050','https://id.example.test','v0.5-user',"
                + "'v0.5 note','kept across update','[\\\"upgrade\\\"]',1,2,3,NULL);\""
              )
              machine.succeed(
                "chown marginalis:marginalis /var/lib/marginalis/marginalis.sqlite && "
                + "chmod 0600 /var/lib/marginalis/marginalis.sqlite"
              )
              machine.succeed("systemctl start marginalis.service")
              machine.wait_until_succeeds(
                "curl -fsS http://127.0.0.1:3000/api/v2/health | jq -e '.status == \"ok\"'"
              )
              machine.succeed(
                "test $(sqlite3 /var/lib/marginalis/marginalis.sqlite "
                + "\"SELECT COUNT(*) FROM notes WHERE note_id = '019f0000-0000-7000-8000-000000000050' "
                + "AND creator_subject = 'v0.5-user' AND title = 'v0.5 note' "
                + "AND body = 'kept across update' AND revision = 3\") -eq 1"
              )
              machine.succeed("systemctl start marginalis-diagnose.service")
              machine.succeed(
                "journalctl -u marginalis-diagnose.service -o cat | "
                + "grep '^{\"status\":\"ok\"' | tail -1 | jq -e "
                + "'.database.schema.ok and .database.schema.actual == 4 and .database.schema.expected == 4'"
              )
            '';
          };

          # 実 Kanidm 1.10、private CA、nginx TLS、subpathを通して、Discoveryと
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
                ];
                services.nginx = {
                  enable = true;
                  virtualHosts."marginalis.example.test" = {
                    forceSSL = true;
                    sslCertificate = "${kanidmDiscoveryCerts}/app-cert.pem";
                    sslCertificateKey = "${kanidmDiscoveryCerts}/app-key.pem";
                    locations."/marginalis/".proxyPass = "http://127.0.0.1:3000/";
                    locations."/.well-known/oauth-authorization-server/marginalis".proxyPass =
                      "http://127.0.0.1:3000/.well-known/oauth-authorization-server/marginalis";
                    locations."/.well-known/oauth-protected-resource/marginalis/mcp".proxyPass =
                      "http://127.0.0.1:3000/.well-known/oauth-protected-resource/marginalis/mcp";
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
                  mcp.enable = true;
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
              app.wait_until_succeeds("curl -fsS http://127.0.0.1:3000/api/v2/health | grep -q '\"api_version\":\"v2\"'")
              app.succeed(
                "curl --cacert ${kanidmDiscoveryCerts}/ca.pem -fsS https://marginalis.example.test/.well-known/oauth-authorization-server/marginalis | ${pkgs.jq}/bin/jq -e '.issuer == \"https://marginalis.example.test/marginalis\"'"
              )
              app.succeed(
                "curl --cacert ${kanidmDiscoveryCerts}/ca.pem -fsS https://marginalis.example.test/.well-known/oauth-protected-resource/marginalis/mcp | ${pkgs.jq}/bin/jq -e '.resource == \"https://marginalis.example.test/marginalis/mcp\"'"
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
              app.succeed(
                "cp ${./tests/browser/kanidm-login.spec.js} /tmp/kanidm-login.spec.js; "
                + "cp ${./tests/browser/webui-editing.spec.js} /tmp/webui-editing.spec.js; "
                + "cd /tmp; "
                + "set +e; playwright test kanidm-login.spec.js webui-editing.spec.js --reporter=line --workers=1 "
                + ">/tmp/playwright-raw.log 2>&1; status=$?; set -e; "
                + "bash ${./.github/scripts/protocol-artifact.sh} sanitize "
                + "/tmp/playwright-raw.log /tmp/playwright.log; "
                + "bash ${./.github/scripts/protocol-artifact.sh} check /tmp/playwright.log; "
                + "cat /tmp/playwright.log; exit $status"
              )
              for operation in ["registration", "authorization", "consent", "revocation"]:
                app.succeed(
                  "journalctl -u marginalis.service "
                  + "| grep 'mcp.oauth.operation.completed' "
                  + f"| grep 'operation=\"{operation}\"'"
                )
              app.succeed(
                "journalctl -u marginalis.service "
                + "| grep 'mcp.oauth.operation.failed' "
                + "| grep 'operation=\"consent\"' "
                + "| grep 'status=403'"
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
          rustToolchain = rustToolchainFor pkgs;
        in
        {
          default = pkgs.mkShell {
            packages = with pkgs; [
              curl
              actionlint
              cargo-audit
              cargo-llvm-cov
              rustToolchain
              cargo-make
              git
              gh
              jq
              lld
              nix
              nixfmt
              nodejs_22
              ripgrep
              sqlite
              wasm-bindgen-cli
            ];

            RUST_SRC_PATH = "${rustToolchain}/lib/rustlib/src/rust/library";
          };
        }
      );

      formatter = forAllSystems (system: (pkgsFor system).nixfmt);
    };
}
