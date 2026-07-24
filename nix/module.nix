self:
{
  config,
  lib,
  pkgs,
  ...
}:

let
  cfg = config.services.marginalis;
  listenPort =
    let
      matched = builtins.match ".*:([0-9]+)$" cfg.listenAddress;
    in
    if matched == null then
      throw "services.marginalis.listenAddress must end with a TCP port"
    else
      lib.toInt (builtins.elemAt matched 0);
  inherit (lib)
    mkEnableOption
    mkIf
    mkOption
    optionalAttrs
    optionals
    types
    ;
in
{
  options.services.marginalis = {
    enable = mkEnableOption "Marginalis research-note server";

    package = mkOption {
      type = types.package;
      default = self.packages.${pkgs.stdenv.hostPlatform.system}.default;
      description = "Marginalis package to execute.";
    };

    listenAddress = mkOption {
      type = types.str;
      default = "127.0.0.1:3000";
      description = "Socket address on which Marginalis accepts HTTP requests.";
    };

    openFirewall = mkOption {
      type = types.bool;
      default = false;
      description = "Whether to allow the TCP port in listenAddress through the NixOS firewall. This does not make a loopback-only listenAddress externally reachable.";
    };

    logFilter = mkOption {
      type = types.str;
      default = "info,marginalis_auth_oidc=info";
      example = "info,marginalis_server=debug,marginalis_auth_oidc=debug";
      description = "RUST_LOG filter for structured tracing output. Do not enable request-body or secret logging.";
    };

    baseUrl = mkOption {
      type = types.nullOr types.str;
      default = null;
      example = "https://marginalis.example.test";
      description = "Public HTTPS Base URL, including any reverse-proxy subpath.";
    };

    dataDir = mkOption {
      type = types.str;
      default = "/var/lib/marginalis";
      description = "Directory holding the SQLite canonical store and its runtime state.";
    };

    backupDirectory = mkOption {
      type = types.nullOr types.str;
      default = null;
      example = "/var/lib/marginalis-backups";
      description = "Absolute directory in which marginalis-backup.service creates timestamped backup generations. Set this only after choosing persistent backup storage and retention outside dataDir.";
    };

    databaseUrl = mkOption {
      type = types.nullOr types.str;
      default = null;
      description = "SQLite connection URL. Defaults to a database below dataDir.";
    };

    oidc = {
      issuerUrl = mkOption {
        type = types.nullOr types.str;
        default = null;
        example = "https://id.sandi05.com/oauth2/openid/marginalis";
        description = "OIDC issuer URL.";
      };

      clientId = mkOption {
        type = types.nullOr types.str;
        default = null;
        example = "marginalis";
        description = "OIDC client ID.";
      };

      clientSecretFile = mkOption {
        type = types.nullOr types.str;
        default = null;
        example = "/run/secrets/marginalis-oidc-client-secret";
        description = "Runtime path to the OIDC client secret. It is passed with systemd credentials, never copied to the Nix store.";
      };
    };

    mcp = {
      enable = mkOption {
        type = types.bool;
        default = false;
        description = "Whether to expose the OAuth-protected MCP endpoint and authorization server.";
      };
    };
  };

  config = mkIf cfg.enable {
    assertions = [
      {
        assertion = cfg.baseUrl != null;
        message = "services.marginalis.baseUrl must be set.";
      }
      {
        assertion = cfg.oidc.issuerUrl != null;
        message = "services.marginalis.oidc.issuerUrl must be set.";
      }
      {
        assertion = cfg.oidc.clientId != null;
        message = "services.marginalis.oidc.clientId must be set.";
      }
      {
        assertion = cfg.oidc.clientSecretFile != null;
        message = "services.marginalis.oidc.clientSecretFile must be set.";
      }
      {
        assertion =
          cfg.backupDirectory == null
          || (
            lib.hasPrefix "/" cfg.backupDirectory
            && cfg.backupDirectory != cfg.dataDir
            && !lib.hasPrefix "${cfg.dataDir}/" cfg.backupDirectory
          );
        message = "services.marginalis.backupDirectory must be an absolute path outside services.marginalis.dataDir.";
      }
    ];

    users.groups.marginalis = { };
    users.users.marginalis = {
      isSystemUser = true;
      group = "marginalis";
    };

    systemd.tmpfiles.rules = [
      "d ${cfg.dataDir} 0750 marginalis marginalis -"
    ]
    ++ optionals (cfg.backupDirectory != null) [
      "d ${cfg.backupDirectory} 0750 marginalis marginalis -"
    ];

    networking.firewall.allowedTCPPorts = optionals cfg.openFirewall [ listenPort ];

    systemd.services.marginalis = {
      description = "Marginalis research-note server";
      wantedBy = [ "multi-user.target" ];
      after = [ "network-online.target" ];
      wants = [ "network-online.target" ];
      environment = {
        RUST_LOG = cfg.logFilter;
        MARGINALIS_BASE_URL = cfg.baseUrl;
        MARGINALIS_LISTEN_ADDR = cfg.listenAddress;
        MARGINALIS_DATA_DIR = cfg.dataDir;
        MARGINALIS_DATABASE_URL =
          if cfg.databaseUrl == null then "sqlite:${cfg.dataDir}/marginalis.sqlite" else cfg.databaseUrl;
        OIDC_ISSUER_URL = cfg.oidc.issuerUrl;
        OIDC_CLIENT_ID = cfg.oidc.clientId;
        OIDC_CLIENT_SECRET_FILE = "%d/oidc-client-secret";
        MARGINALIS_MCP_ENABLE = if cfg.mcp.enable then "true" else "false";
      };
      serviceConfig = {
        ExecStart = "${cfg.package}/bin/marginalis";
        User = "marginalis";
        Group = "marginalis";
        WorkingDirectory = cfg.dataDir;
        Restart = "on-failure";
        RestartSec = "5s";
        LoadCredential = [
          "oidc-client-secret:${cfg.oidc.clientSecretFile}"
        ];
        NoNewPrivileges = true;
        CapabilityBoundingSet = "";
        PrivateTmp = true;
        ProtectHome = true;
        ProtectSystem = "strict";
        ProtectKernelTunables = true;
        RestrictAddressFamilies = [
          "AF_UNIX"
          "AF_INET"
          "AF_INET6"
        ];
        SystemCallFilter = [
          "@system-service"
          "~@privileged"
        ];
        ReadWritePaths = [ cfg.dataDir ];
      }
      // optionalAttrs (cfg.dataDir == "/var/lib/marginalis") {
        # 既定の永続領域はservice開始前にsystemd自身が作成する。手動削除後も
        # ReadWritePathsのmount namespace構築より先に復元される。
        StateDirectory = "marginalis";
        StateDirectoryMode = "0750";
      };
    };

    # v0.3の削除済みノートは30日間だけ保持する。SQLite正本だけを操作するため、HTTP serviceを
    # 停止せずに日次実行できる。
    systemd.services.marginalis-purge-deleted = {
      description = "Purge expired Marginalis soft-deleted notes";
      environment = {
        RUST_LOG = cfg.logFilter;
        MARGINALIS_DATA_DIR = cfg.dataDir;
        MARGINALIS_DATABASE_URL =
          if cfg.databaseUrl == null then "sqlite:${cfg.dataDir}/marginalis.sqlite" else cfg.databaseUrl;
      };
      serviceConfig = {
        Type = "oneshot";
        ExecStart = "${cfg.package}/bin/marginalis purge-deleted";
        User = "marginalis";
        Group = "marginalis";
        WorkingDirectory = cfg.dataDir;
        NoNewPrivileges = true;
        CapabilityBoundingSet = "";
        PrivateTmp = true;
        ProtectHome = true;
        ProtectSystem = "strict";
        RestrictAddressFamilies = [ "AF_UNIX" ];
        SystemCallFilter = [ "@system-service" "~@privileged" ];
        ReadWritePaths = [ cfg.dataDir ];
      }
      // optionalAttrs (cfg.dataDir == "/var/lib/marginalis") {
        StateDirectory = "marginalis";
        StateDirectoryMode = "0750";
      };
    };

    systemd.timers.marginalis-purge-deleted = {
      description = "Purge expired Marginalis soft-deleted notes daily";
      wantedBy = [ "timers.target" ];
      timerConfig = {
        OnCalendar = "daily";
        Persistent = true;
        Unit = "marginalis-purge-deleted.service";
      };
    };

    # backup先は運用者が永続storageとretentionを決めてから明示する。timerは提供しない。
    # このunitはHTTP serverと競合させ、SQLite正本から一貫したv3 archiveを取得する。
    systemd.services.marginalis-backup = mkIf (cfg.backupDirectory != null) {
      description = "Create a consistent Marginalis backup";
      conflicts = [ "marginalis.service" ];
      environment = {
        RUST_LOG = cfg.logFilter;
        MARGINALIS_DATA_DIR = cfg.dataDir;
        MARGINALIS_DATABASE_URL =
          if cfg.databaseUrl == null then "sqlite:${cfg.dataDir}/marginalis.sqlite" else cfg.databaseUrl;
      };
      serviceConfig = {
        Type = "oneshot";
        ExecStart = "${cfg.package}/bin/marginalis backup --directory ${lib.escapeShellArg cfg.backupDirectory}";
        User = "marginalis";
        Group = "marginalis";
        WorkingDirectory = cfg.dataDir;
        NoNewPrivileges = true;
        CapabilityBoundingSet = "";
        PrivateTmp = true;
        ProtectHome = true;
        ProtectSystem = "strict";
        ProtectKernelTunables = true;
        RestrictAddressFamilies = [
          "AF_UNIX"
          "AF_INET"
          "AF_INET6"
        ];
        SystemCallFilter = [
          "@system-service"
          "~@privileged"
        ];
        ReadWritePaths = [
          cfg.dataDir
          cfg.backupDirectory
        ];
      }
      // optionalAttrs (cfg.dataDir == "/var/lib/marginalis") {
        StateDirectory = "marginalis";
        StateDirectoryMode = "0750";
      };
    };
  };
}
