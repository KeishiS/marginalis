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
      # AdocWeave v0.6.1 が要求する Rust 1.97.1 を確定的にピンする。
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
            url = "https://raw.githubusercontent.com/KeishiS/AdocWeave/2a7ec4f7c2df6104ead9a7285ca13fc364ce8dda/fixtures/conformance/cases.json";
            hash = "sha256-Mlx66KZinQKdFGkFngC4hJKXKZ7VYGnhEelI8u3lLFg=";
          };
        in
        {
          default = rustPlatform.buildRustPackage {
            pname = "marginalis";
            version = "0.3.0";
            src = ./.;
            cargoLock = {
              lockFile = ./Cargo.lock;
              outputHashes = {
                "adocweave-0.6.1" = "sha256-FEjYbbpKsk3k5u1NucINXho/Z0Pl6OOFFI8xhTJCIv4=";
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
                mv $out-key.pem $out/key.pem
                mv $out-cert.pem $out/cert.pem
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
                machine.succeed("systemctl show -p ActiveState --value marginalis.service | grep -qx inactive")
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
                "sqlite3 /var/lib/marginalis/marginalis.sqlite \"INSERT INTO notes VALUES "
                + "('019f0000-0000-7000-8000-000000000001','https://id.example.test','stale','stale','body','[]',0,0,1,0),"
                + "('019f0000-0000-7000-8000-000000000002','https://id.example.test','recent','recent','body','[]',0,0,1,4102444800000);"
                + "INSERT INTO note_acl VALUES "
                + "('019f0000-0000-7000-8000-000000000001','https://id.example.test','stale',3),"
                + "('019f0000-0000-7000-8000-000000000002','https://id.example.test','recent',3);\""
              )
              machine.succeed("systemctl start marginalis-purge-deleted.service")
              machine.succeed(
                "test $(sqlite3 /var/lib/marginalis/marginalis.sqlite "
                + "\"SELECT COUNT(*) FROM notes WHERE note_id = '019f0000-0000-7000-8000-000000000001'\") -eq 0"
              )
              machine.succeed(
                "test $(sqlite3 /var/lib/marginalis/marginalis.sqlite "
                + "\"SELECT COUNT(*) FROM notes WHERE note_id = '019f0000-0000-7000-8000-000000000002'\") -eq 1"
              )
              machine.succeed("systemctl is-enabled marginalis-purge-deleted.timer")
              machine.succeed("systemctl start marginalis-backup.service")
              machine.succeed(
                "test $(find /var/lib/marginalis-backups/test -mindepth 1 -maxdepth 1 -type d | wc -l) -eq 1"
              )
              machine.succeed(
                "backup=$(find /var/lib/marginalis-backups/test -mindepth 1 -maxdepth 1 -type d); "
                + "test -f \"$backup/COMPLETE\"; "
                + "test -f \"$backup/marginalis-archive.json\"; "
                + "jq -e '.format == \"marginalis-archive-1\" and (.notes | length == 1)' "
                + "\"$backup/marginalis-archive.json\"; "
                + "test $(stat -c %a \"$backup\") = 700; "
                + "test $(stat -c %a \"$backup/COMPLETE\") = 600; "
                + "test $(stat -c %a \"$backup/marginalis-archive.json\") = 600"
              )
              machine.succeed("systemctl start marginalis.service")
              machine.wait_for_unit("marginalis.service")
            '';
          };

          # 実 Kanidm 1.10 の TLS Discovery を通して Marginalis が起動することを確認する。
          # Authorization Code の browser interaction と group変更は別の手動受入/E2Eで扱う。
          kanidm-discovery-vm = pkgs.testers.nixosTest {
            name = "marginalis-kanidm-discovery";
            nodes.idp = {
              services.kanidm = {
                package = pkgs.kanidmWithSecretProvisioning_1_10;
                server = {
                  enable = true;
                  settings = {
                    origin = "https://id.example.test:8443";
                    domain = "id.example.test";
                    bindaddress = "0.0.0.0:8443";
                    tls_chain = "${kanidmDiscoveryCerts}/cert.pem";
                    tls_key = "${kanidmDiscoveryCerts}/key.pem";
                  };
                };
                provision = {
                  enable = true;
                  instanceUrl = "https://localhost:8443";
                  acceptInvalidCerts = true;
                  systems.oauth2.marginalis = {
                    displayName = "Marginalis test client";
                    originUrl = "https://marginalis.example.test/marginalis";
                    originLanding = "https://marginalis.example.test/marginalis";
                    basicSecretFile = pkgs.writeText "marginalis-test-oidc-secret" "test-only-secret";
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
                security.pki.certificateFiles = [ "${kanidmDiscoveryCerts}/ca.pem" ];
                environment.etc."marginalis-test/oidc-client-secret".text = "test-only-secret";
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
              app.start()
              app.wait_for_unit("marginalis.service")
              app.wait_until_succeeds("curl -fsS http://127.0.0.1:3000/api/v2/health | grep -q '\"api_version\":\"v2\"'")
              app.succeed(
                "curl -fsS http://127.0.0.1:3000/.well-known/oauth-authorization-server/marginalis | ${pkgs.jq}/bin/jq -e '.issuer == \"https://marginalis.example.test/marginalis\"'"
              )
              app.succeed(
                "curl -fsS http://127.0.0.1:3000/.well-known/oauth-protected-resource/marginalis/mcp | ${pkgs.jq}/bin/jq -e '.resource == \"https://marginalis.example.test/marginalis/mcp\"'"
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
              cargo-llvm-cov
              rustToolchain
              cargo-make
              git
              gh
              jq
              lld
              nix
              nixfmt
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
