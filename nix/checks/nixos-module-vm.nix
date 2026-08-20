# 実サーバーの代わりに引数と環境を検査するprobeを起動し、moduleが生成するsystemd
# unit、backup、restore-checkの配線をVM上で確かめる。
{ pkgs, self }:
let
  probeServer = pkgs.writeShellApplication {
    name = "marginalis";
    text = ''
      test "$PWD" = "/var/lib/marginalis"
      test "$RUST_LOG" = "info,marginalis_service::oidc=info"
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
      if [ "''${1-}" = "migrate-database" ]; then
        test "$2" = "--directory"
        test "$3" = "/var/lib/marginalis-backups/test"
        touch "$3/migration-completed"
        exit 0
      fi
      test -s "$MARGINALIS_OIDC_CLIENT_SECRET_FILE"
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
    machine.fail("systemctl is-enabled --quiet marginalis-migrate-database.service")
    machine.succeed("systemctl start marginalis-migrate-database.service")
    machine.succeed("test -f /var/lib/marginalis-backups/test/migration-completed")
    machine.succeed("systemctl start marginalis.service")
    machine.wait_for_unit("marginalis.service")
    machine.succeed("systemctl is-enabled marginalis-backup.timer")
    machine.succeed("systemctl is-enabled marginalis-restore-check.timer")
    machine.succeed("systemctl is-active marginalis.service")
  '';
}
