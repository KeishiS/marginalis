{
  description = "Development environment for Marginalis";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  inputs.rust-overlay = {
    url = "github:oxalica/rust-overlay";
    inputs.nixpkgs.follows = "nixpkgs";
  };
  inputs.adocweave = {
    url = "github:KeishiS/adocweave/f45bcd41199e6cb6471fe760ab51883751ba76b7";
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
      # AdocWeaveのtextlint用Processorが要求するNode.js 24.19.0以上へ揃える。
      pnpmFor = pkgs: pkgs.pnpm_11.override { nodejs-slim = pkgs.nodejs-slim_24; };
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
            # Tailwindはglobals.cssの@sourceで、Rust側が生成するHTMLのclassも
            # 走査する。frontendディレクトリーだけを入力にすると@sourceの参照先が
            # ビルド環境に存在せず、共通レイアウトの規則がCSSから欠ける(#487)。
            src = pkgs.lib.fileset.toSource {
              root = ./.;
              fileset = pkgs.lib.fileset.unions [
                ./frontend
                ./crates/marginalis-web/src/http
              ];
            };
            sourceRoot = "source/frontend";
            pnpmDeps = pkgs.fetchPnpmDeps {
              inherit (finalAttrs)
                pname
                version
                ;
              src = ./frontend;
              inherit pnpm;
              fetcherVersion = 4;
              hash = "sha256-5mT5jwLmeeA8tTgaNB6REjKvqR4+1r3CtTPbNeH1tmk=";
            };
            nativeBuildInputs = [
              pkgs.nodejs_24
              pkgs.pnpmConfigHook
              pnpm
            ];
            buildPhase = ''
              runHook preBuild
              pnpm build
              runHook postBuild
            '';
            # @sourceの参照先がビルド入力に含まれない構成変更を検出する。
            # 参照先が無いままだと該当classの規則が静かに欠けるため、ここで失敗させる。
            postBuild = ''
              while IFS= read -r referenced; do
                if [ ! -e "src/styles/$referenced" ]; then
                  echo "@sourceの参照先がビルド入力にありません: $referenced" >&2
                  echo "flake.nixのfrontend srcのfilesetへ追加してください。" >&2
                  exit 1
                fi
              done < <(sed -n 's/^@source "\(.*\)";$/\1/p' src/styles/*.css)
              if ! grep -qF 'max-width:var(--content-width)' dist/assets/*.css; then
                echo "共通レイアウトの幅規則がCSSに含まれていません(#487)。" >&2
                exit 1
              fi
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
                "adocweave-${adocweaveVersion}" = "sha256-QlqWAlkHL00wU5OOrUHmwdBjsY4WRcq7rFNypaquJg0=";
                "mcp-authorization-server-0.1.0" = "sha256-pXrn8DUKm6Y4/8MCWeojVs3+w6eTQMjoBiv1OFNZUh8=";
                "mcp-authorization-server-cimd-0.1.0" = "sha256-pXrn8DUKm6Y4/8MCWeojVs3+w6eTQMjoBiv1OFNZUh8=";
                "oidc-browser-login-0.2.0" = "sha256-Dk5uE7ZzH8zacNbdMSoleb4V8ZBOa75WLGMOCxt2Knc=";
                "oidc-browser-login-testkit-0.2.0" = "sha256-Dk5uE7ZzH8zacNbdMSoleb4V8ZBOa75WLGMOCxt2Knc=";
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
          }
        )
      );

      devShells = forAllSystems (
        system:
        let
          pkgs = pkgsFor system;
          pnpm = pnpmFor pkgs;
          rustToolchain = rustToolchainFor pkgs;
          # AdocWeaveはNix packageの公開先をLinuxに限定している。0.47.0でoverlayと
          # 製品別のpackage属性が廃止され、他のsystem向けにderivationを組み立てる
          # 手段がなくなったため、Linux以外のdevShellにはCLIを含めない。文書検査
          # (cargo make docs-check)はLinuxで実行する。
          adocweaveCli = pkgs.lib.optionals (adocweave.packages ? ${system}) [
            adocweave.packages.${system}.default
          ];
        in
        {
          default = pkgs.mkShell {
            packages =
              adocweaveCli
              ++ (with pkgs; [
                curl
                actionlint
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
                nodejs_24
                pnpm
                noto-fonts-cjk-sans
                playwright-driver.browsers
                playwright-test
                ripgrep
                sqlite
              ]);

            RUST_SRC_PATH = "${rustToolchain}/lib/rustlib/src/rust/library";
            FONTCONFIG_FILE = "${pkgs.fontconfig.out}/etc/fonts/fonts.conf";
            FONTCONFIG_PATH = "${pkgs.fontconfig.out}/etc/fonts";
          };
        }
      );

      formatter = forAllSystems (system: (pkgsFor system).nixfmt);
    };
}
