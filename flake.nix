{
  description = "Development environment for Marginalis";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs =
    { self, nixpkgs, ... }:
    let
      systems = [
        "aarch64-darwin"
        "aarch64-linux"
        "x86_64-darwin"
        "x86_64-linux"
      ];
      forAllSystems = nixpkgs.lib.genAttrs systems;
      pkgsFor = system: import nixpkgs { inherit system; };
    in
    {
      packages = forAllSystems (
        system:
        let
          pkgs = pkgsFor system;
        in
        {
          default = pkgs.rustPlatform.buildRustPackage {
            pname = "marginalis";
            version = "0.1.0";
            src = ./.;
            cargoLock = {
              lockFile = ./Cargo.lock;
              outputHashes = {
                "adocweave-0.1.0-rc.3" = "sha256-DvIaIEdTr7e0I9pRrm8W0bwtCceLGxwHbouxhbwibDY=";
              };
            };
            cargoBuildFlags = [
              "--package"
              "marginalis-web"
            ];
            doCheck = false;
            installPhase = ''
              install -Dm755 target/release/marginalis-web $out/bin/marginalis
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
            pkgs.writeText "marginalis-nixos-module-evaluation" evaluated.config.systemd.services.marginalis.serviceConfig.ExecStart;
        }
      );

      devShells = forAllSystems (
        system:
        let
          pkgs = pkgsFor system;
        in
        {
          default = pkgs.mkShell {
            packages = with pkgs; [
              actionlint
              cargo
              cargo-make
              clippy
              git
              lld
              nix
              nixfmt
              ripgrep
              rust-analyzer
              rustc
              rustfmt
              sqlite
              wasm-bindgen-cli
            ];

            RUST_SRC_PATH = "${pkgs.rustPlatform.rustLibSrc}";
          };
        }
      );

      formatter = forAllSystems (system: (pkgsFor system).nixfmt);
    };
}
