# flake.nixのchecksを責務単位に分割したentry point。check名と検査内容は
# flake.nixへ直書きしていた頃と同じに保つ。
{
  pkgs,
  self,
  system,
  nixpkgs,
  rustPlatform,
  version,
  adocweaveVersion,
}:
{
  schema9-archive-migration = import ./schema9-archive-migration.nix {
    inherit
      pkgs
      self
      system
      rustPlatform
      adocweaveVersion
      ;
  };
  nixos-module = import ./nixos-module.nix {
    inherit
      pkgs
      self
      system
      nixpkgs
      ;
  };
  nixos-module-vm = import ./nixos-module-vm.nix { inherit pkgs self; };
  nixos-module-runtime-vm = import ./nixos-module-runtime-vm.nix {
    inherit
      pkgs
      self
      system
      version
      adocweaveVersion
      ;
  };
  mcp-authorization-vm = import ./mcp-authorization-vm.nix { inherit pkgs self; };
  kanidm-discovery-vm = import ./kanidm-discovery-vm.nix { inherit pkgs self; };
}
