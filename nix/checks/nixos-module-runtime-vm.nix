# 実バイナリーをVMで起動し、health、fail closedなlogin、期限切れ削除、backup、
# restore-check、診断、旧schema検出までの運用経路を通しで確かめる。
{
  pkgs,
  self,
  system,
  version,
  adocweaveVersion,
}:
let
  marginalisV050Schema = pkgs.fetchurl {
    url = "https://raw.githubusercontent.com/KeishiS/marginalis/v0.5.0/crates/marginalis-sqlite/src/schema.sql";
    hash = "sha256-U8R8xzBYkohX+zKr3TtLlmvTPMhif+EBylhF+2L9u64=";
  };
in
pkgs.testers.nixosTest {
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
      "test \"$(/run/current-system/sw/bin/marginalis --version)\" = 'marginalis ${version}'"
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
      "sqlite3 /var/lib/marginalis/marginalis.sqlite < ${./fixtures/runtime-purge-seed.sql}"
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
      + "jq -e '.format == \"marginalis-archive-17\" "
      + "and .adocweave_package_version == \"${adocweaveVersion}\" "
      + "and .note_profile_version == 5 and (.notes | length == 1)' "
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
      + "'.database.schema.ok == false and .database.schema.actual == 1 and .database.schema.expected == 21'"
    )
    machine.succeed(
      "runuser -u marginalis -- sqlite3 /var/lib/marginalis/marginalis.sqlite "
      + "'UPDATE schema_migrations SET version = 13; PRAGMA journal_mode=WAL'"
    )
    machine.succeed("rm -f /var/lib/marginalis/marginalis.sqlite*")
    machine.succeed(
      "sqlite3 /var/lib/marginalis/marginalis.sqlite < ${./fixtures/schema4-migrations.sql}"
    )
    machine.succeed(
      "sqlite3 /var/lib/marginalis/marginalis.sqlite < ${marginalisV050Schema}"
    )
    machine.succeed(
      "sqlite3 /var/lib/marginalis/marginalis.sqlite "
      + "\"UPDATE schema_migrations SET version = 5;\""
    )
    machine.succeed(
      "sqlite3 /var/lib/marginalis/marginalis.sqlite < ${./fixtures/schema5-note-seed.sql}"
    )
    machine.succeed(
      "chown marginalis:marginalis /var/lib/marginalis/marginalis.sqlite && "
      + "chmod 0600 /var/lib/marginalis/marginalis.sqlite"
    )
    machine.execute("systemctl start marginalis.service")
    machine.wait_until_succeeds(
      "timeout 5s journalctl --no-pager -u marginalis.service -o cat | "
      + "grep -F 'unsupported database schema version 5; expected 21'"
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
          + "and .database.schema.expected == 21 "
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
}
