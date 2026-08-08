# NixOS moduleを有効・無効の両構成で評価し、firewall、環境変数、systemd unitの
# 宣言が期待どおりであることをVMなしで確かめる。
{
  pkgs,
  self,
  system,
  nixpkgs,
}:
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
          };
        };
      }
    ];
  };
in
assert evaluated.config.networking.firewall.allowedTCPPorts == [ 3000 ];
assert evaluated.config.systemd.services.marginalis.environment.MARGINALIS_MCP_ENABLE == "true";
assert
  evaluated.config.systemd.services.marginalis-diagnose.environment.MARGINALIS_MCP_ENABLE == "true";
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
pkgs.writeText "marginalis-nixos-module-evaluation" evaluated.config.systemd.services.marginalis.serviceConfig.ExecStart
