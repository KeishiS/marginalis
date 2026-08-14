{
  description = "Development environment for Marginalis";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  inputs.rust-overlay = {
    url = "github:oxalica/rust-overlay";
    inputs.nixpkgs.follows = "nixpkgs";
  };
  inputs.adocweave = {
    url = "github:KeishiS/adocweave/0b97d24adc3ad241011621933cd6400ad17bba74";
    inputs.nixpkgs.follows = "nixpkgs";
    inputs.rust-overlay.follows = "rust-overlay";
  };

  outputs =
    {
      self,
      nixpkgs,
      rust-overlay,
      adocweave,
      ...
    }:
    let
      systems = [
        "aarch64-darwin"
        "aarch64-linux"
        "x86_64-linux"
      ];
      packageSystems = [
        "aarch64-linux"
        "x86_64-linux"
      ];
      workspacePackage = (builtins.fromTOML (builtins.readFile ./Cargo.toml)).workspace.package;
      version = workspacePackage.version;
      rustVersion = workspacePackage.rust-version;
      # AdocWeaveの版の正本はCargo.lockの解決結果とする。cargoLock.outputHashesの鍵と
      # checksの期待値をここから導出し、版の直書きを残さない。
      adocweaveVersion =
        (nixpkgs.lib.findFirst (package: package.name == "adocweave") null
          (builtins.fromTOML (builtins.readFile ./Cargo.lock)).package
        ).version;
      forAllSystems = nixpkgs.lib.genAttrs systems;
      pkgsFor =
        system:
        import nixpkgs {
          inherit system;
          overlays = [ rust-overlay.overlays.default ];
        };
      pnpmFor = pkgs: pkgs.pnpm_11.override { nodejs-slim = pkgs.nodejs-slim_22; };
      # Cargo manifestが宣言する最低Rust版を、開発環境と配布buildでも使用する。
      rustToolchainFor =
        pkgs:
        pkgs.rust-bin.stable.${rustVersion}.default.override {
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
      packages = nixpkgs.lib.genAttrs packageSystems (
        system:
        let
          pkgs = pkgsFor system;
          pnpm = pnpmFor pkgs;
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
              hash = "sha256-HnSDhWKtgL3zIZwzou8LjrSUMr/Z41Qap8OgmkNbpIc=";
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
                ./tools/marginalis-documentation
              ];
            };
            cargoLock = {
              lockFile = ./Cargo.lock;
              outputHashes = {
                "adocweave-${adocweaveVersion}" = "sha256-na03cwua6oPnAFmvGcNSIXes/R7rXzChEaDOwg0h4DI=";
                "mcp-authorization-server-0.1.0" = "sha256-pXrn8DUKm6Y4/8MCWeojVs3+w6eTQMjoBiv1OFNZUh8=";
                "mcp-authorization-server-cimd-0.1.0" = "sha256-pXrn8DUKm6Y4/8MCWeojVs3+w6eTQMjoBiv1OFNZUh8=";
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
        in
        # NixOS VMを使う検査はLinuxに限る。検査の定義はnix/checks/へ責務単位で分割する。
        pkgs.lib.optionalAttrs pkgs.stdenv.isLinux (
          import ./nix/checks {
            inherit
              pkgs
              self
              system
              nixpkgs
              version
              adocweaveVersion
              ;
            rustPlatform = rustPlatformFor pkgs;
          }
        )
      );

      devShells = forAllSystems (
        system:
        let
          pkgs = (pkgsFor system).extend adocweave.overlays.default;
          pnpm = pnpmFor pkgs;
          rustToolchain = rustToolchainFor pkgs;
          # AdocWeaveはmacOS向けCLIも配布しているが、Nix packageの公開先は
          # Linuxに限定されている。移行期間中の文書検査を全開発環境で同じに
          # するため、同じderivationを対応済みのUnix環境でも評価する。
          adocweavePackage = pkgs.adocweave.overrideAttrs (previous: {
            meta = previous.meta // {
              platforms = pkgs.lib.platforms.unix;
            };
          });
        in
        {
          default = pkgs.mkShell {
            packages = with pkgs; [
              curl
              actionlint
              adocweavePackage
              cargo-audit
              cargo-machete
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
