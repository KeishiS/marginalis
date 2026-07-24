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
            version = "0.2.0";
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
                        membershipApiUrl = "https://id.example.test";
                        membershipTokenFile = "/run/secrets/marginalis-membership-token";
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
                  test -d "$MARGINALIS_DATA_DIR"
                  test "$RUST_LOG" = "info,marginalis_auth_oidc=info"
                  if [ "''${1-}" = "backup" ] && [ "''${2-}" = "--directory" ]; then
                    test "$3" = "/var/lib/marginalis-backups/test"
                    touch "$3/backup-created"
                    exit 0
                  fi
                  test -s "$OIDC_CLIENT_SECRET_FILE"
                  test -s "$KANIDM_MEMBERSHIP_TOKEN_FILE"
                  touch "$MARGINALIS_DATA_DIR/service-started"
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
                environment.etc."marginalis-test/membership-token".text = "test-only-membership-token";

                services.marginalis = {
                  enable = true;
                  package = probeServer;
                  baseUrl = "https://marginalis.example.test";
                  backupDirectory = "/var/lib/marginalis-backups/test";
                  oidc = {
                    issuerUrl = "https://id.example.test";
                    clientId = "marginalis";
                    clientSecretFile = "/etc/marginalis-test/oidc-client-secret";
                    membershipApiUrl = "https://id.example.test";
                    membershipTokenFile = "/etc/marginalis-test/membership-token";
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
              environment.etc."marginalis-test/membership-token".text = "test-only-membership-token";

              services.marginalis = {
                enable = true;
                baseUrl = "https://marginalis.example.test";
                oidc = {
                  # networkに依存せずroot-only縮退起動を検証する。実OIDCの確認は手動acceptanceで行う。
                  issuerUrl = "https://127.0.0.1:1";
                  clientId = "marginalis";
                  clientSecretFile = "/etc/marginalis-test/oidc-client-secret";
                  membershipApiUrl = "https://127.0.0.1:1";
                  membershipTokenFile = "/etc/marginalis-test/membership-token";
                };
              };
            };

            testScript = ''
              machine.wait_for_unit("marginalis.service")
              machine.wait_until_succeeds(
                  "curl -fsS http://127.0.0.1:3000/api/v2/health | jq -e '.status == \"ok\" and .api_version == \"v2\"'"
              )
              machine.succeed(
                  "curl -fsS http://127.0.0.1:3000/api/v2/openapi.json | jq -e '.openapi == \"3.1.0\"'"
              )
              machine.succeed("sqlite3 /var/lib/marginalis/marginalis.sqlite 'SELECT 1 FROM v3_notes'")
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
