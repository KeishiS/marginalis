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
  authorizationValues = with cfg.mcp.authorization; [
    issuer
    upstreamIssuerClaim
    upstreamSubjectClaim
    groupsClaim
  ];
  authorizationEnabled = builtins.all (value: value != null) authorizationValues;
  authorizationUnset = builtins.all (value: value == null) authorizationValues;
  authorizationEnvironment = optionalAttrs authorizationEnabled {
    MARGINALIS_MCP_AUTHORIZATION_ISSUER = cfg.mcp.authorization.issuer;
    MARGINALIS_MCP_UPSTREAM_ISSUER_CLAIM = cfg.mcp.authorization.upstreamIssuerClaim;
    MARGINALIS_MCP_UPSTREAM_SUBJECT_CLAIM = cfg.mcp.authorization.upstreamSubjectClaim;
    MARGINALIS_MCP_GROUPS_CLAIM = cfg.mcp.authorization.groupsClaim;
  };
  commonServiceConfig = {
    User = "marginalis";
    Group = "marginalis";
    UMask = "0077";
    NoNewPrivileges = true;
    CapabilityBoundingSet = "";
    LockPersonality = true;
    PrivateDevices = true;
    PrivateTmp = true;
    ProtectHome = true;
    ProtectSystem = "strict";
    ProtectClock = true;
    ProtectControlGroups = true;
    ProtectKernelLogs = true;
    ProtectKernelModules = true;
    ProtectKernelTunables = true;
    RestrictNamespaces = true;
    RestrictRealtime = true;
    SystemCallFilter = [
      "@system-service"
      "~@privileged"
    ];
  };
  localServiceConfig = commonServiceConfig // {
    RestrictAddressFamilies = [ "AF_UNIX" ];
  };
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
      example = "info,marginalis_application=debug,marginalis_auth_oidc=debug";
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

    backupRetention = mkOption {
      type = types.ints.positive;
      default = 30;
      description = "Number of verified successful backup generations to retain. Incomplete and unrecognized entries are never counted or removed.";
    };

    restoreCheck = {
      enable = mkEnableOption "quarterly isolated restore verification";

      calendar = mkOption {
        type = types.str;
        default = "Sat *-01,04,07,10-01..07 03:00:00";
        description = "systemd OnCalendar expression for explicitly enabled isolated restore verification. The default is the first Saturday of each quarter.";
      };
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

      caCertificateFile = mkOption {
        type = types.nullOr types.str;
        default = null;
        example = "/run/secrets/internal-ca.pem";
        description = "Optional PEM CA certificate for a private Kanidm TLS PKI, used for OIDC discovery and token exchange.";
      };
    };

    mcp = {
      enable = mkOption {
        type = types.bool;
        default = false;
        description = "Whether to expose the MCP resource protected by an external Authorization Server.";
      };

      allowedOrigins = mkOption {
        type = types.listOf types.str;
        default = [ ];
        description = "Exact HTTPS browser origins permitted to call only the MCP endpoint. Native MCP clients omit Origin and use Bearer authentication.";
      };

      authorization = {
        issuer = mkOption {
          type = types.nullOr types.str;
          default = null;
          example = "https://evaluation.jp.auth0.com/";
          description = "Authorization Server issuer used to discover signing keys and validate MCP access tokens.";
        };

        upstreamIssuerClaim = mkOption {
          type = types.nullOr types.str;
          default = null;
          example = "https://notes.example.test/claims/upstream-issuer";
          description = "Namespaced access-token claim containing the verified upstream OIDC issuer.";
        };

        upstreamSubjectClaim = mkOption {
          type = types.nullOr types.str;
          default = null;
          example = "https://notes.example.test/claims/upstream-subject";
          description = "Namespaced access-token claim containing the verified upstream OIDC subject.";
        };

        groupsClaim = mkOption {
          type = types.nullOr types.str;
          default = null;
          example = "https://notes.example.test/claims/groups";
          description = "Namespaced access-token claim containing the verified upstream group array.";
        };
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
          lib.hasPrefix "/" cfg.dataDir
          && cfg.dataDir != "/"
          && builtins.match ".*[[:space:]].*" cfg.dataDir == null;
        message = "services.marginalis.dataDir must be an absolute non-root path without whitespace.";
      }
      {
        assertion = cfg.oidc.clientSecretFile == null || lib.hasPrefix "/" cfg.oidc.clientSecretFile;
        message = "services.marginalis.oidc.clientSecretFile must be an absolute path.";
      }
      {
        assertion = cfg.oidc.caCertificateFile == null || lib.hasPrefix "/" cfg.oidc.caCertificateFile;
        message = "services.marginalis.oidc.caCertificateFile must be an absolute path when set.";
      }
      {
        assertion = authorizationUnset || authorizationEnabled;
        message = "services.marginalis.mcp.authorization options must be all set or all unset.";
      }
      {
        assertion = !cfg.mcp.enable || authorizationEnabled;
        message = "services.marginalis.mcp.authorization options must be set when MCP is enabled.";
      }
      {
        assertion = !authorizationEnabled || cfg.mcp.enable;
        message = "services.marginalis.mcp.enable must be true when authorization is set.";
      }
      {
        assertion =
          cfg.backupDirectory == null
          || (
            lib.hasPrefix "/" cfg.backupDirectory
            && cfg.backupDirectory != "/"
            && builtins.match ".*[[:space:]].*" cfg.backupDirectory == null
            && cfg.backupDirectory != cfg.dataDir
            && !lib.hasPrefix "${cfg.dataDir}/" cfg.backupDirectory
            && !lib.hasPrefix "${cfg.backupDirectory}/" cfg.dataDir
          );
        message = "services.marginalis.backupDirectory and services.marginalis.dataDir must be separate absolute non-root paths without whitespace; neither may contain the other.";
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
        MARGINALIS_DATABASE_URL = "sqlite:${cfg.dataDir}/marginalis.sqlite";
        OIDC_ISSUER_URL = cfg.oidc.issuerUrl;
        OIDC_CLIENT_ID = cfg.oidc.clientId;
        OIDC_CLIENT_SECRET_FILE = "%d/oidc-client-secret";
        OIDC_CA_CERTIFICATE_FILE =
          if cfg.oidc.caCertificateFile == null then "" else cfg.oidc.caCertificateFile;
        MARGINALIS_MCP_ENABLE = if cfg.mcp.enable then "true" else "false";
        MARGINALIS_MCP_ALLOWED_ORIGINS = lib.concatStringsSep "," cfg.mcp.allowedOrigins;
      }
      // authorizationEnvironment;
      serviceConfig =
        commonServiceConfig
        // {
          ExecStart = "${cfg.package}/bin/marginalis";
          WorkingDirectory = cfg.dataDir;
          Restart = "on-failure";
          RestartSec = "5s";
          TimeoutStopSec = "30s";
          LoadCredential = [ "oidc-client-secret:${cfg.oidc.clientSecretFile}" ];
          RestrictAddressFamilies = [
            "AF_UNIX"
            "AF_INET"
            "AF_INET6"
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

    # v0.3の削除済みノートは30日間だけ保持し、期限切れの認証状態も削除する。SQLite正本だけを
    # 操作するため、HTTP serviceを停止せずに日次実行できる。
    systemd.services.marginalis-purge-expired = {
      description = "Purge expired Marginalis notes and authentication state";
      environment = {
        RUST_LOG = cfg.logFilter;
        MARGINALIS_DATABASE_URL = "sqlite:${cfg.dataDir}/marginalis.sqlite";
      };
      serviceConfig =
        localServiceConfig
        // {
          Type = "oneshot";
          ExecStart = "${cfg.package}/bin/marginalis purge-expired";
          WorkingDirectory = cfg.dataDir;
          ReadWritePaths = [ cfg.dataDir ];
        }
        // optionalAttrs (cfg.dataDir == "/var/lib/marginalis") {
          StateDirectory = "marginalis";
          StateDirectoryMode = "0750";
        };
    };

    systemd.timers.marginalis-purge-expired = {
      description = "Purge expired Marginalis state daily";
      wantedBy = [ "timers.target" ];
      timerConfig = {
        OnCalendar = "daily";
        Persistent = true;
        Unit = "marginalis-purge-expired.service";
      };
    };

    # secretやnetworkを必要とせず、SQLiteを変更しない運用診断。
    systemd.services.marginalis-diagnose = {
      description = "Diagnose the Marginalis SQLite database and public configuration";
      environment = {
        RUST_LOG = cfg.logFilter;
        MARGINALIS_DATABASE_URL = "sqlite:${cfg.dataDir}/marginalis.sqlite";
        MARGINALIS_BASE_URL = cfg.baseUrl;
        MARGINALIS_LISTEN_ADDR = cfg.listenAddress;
        OIDC_ISSUER_URL = cfg.oidc.issuerUrl;
        OIDC_CLIENT_ID = cfg.oidc.clientId;
        OIDC_CA_CERTIFICATE_FILE =
          if cfg.oidc.caCertificateFile == null then "" else cfg.oidc.caCertificateFile;
        MARGINALIS_MCP_ENABLE = if cfg.mcp.enable then "true" else "false";
        MARGINALIS_MCP_ALLOWED_ORIGINS = lib.concatStringsSep "," cfg.mcp.allowedOrigins;
      }
      // authorizationEnvironment;
      serviceConfig = localServiceConfig // {
        Type = "oneshot";
        ExecStart = "${cfg.package}/bin/marginalis diagnose";
        WorkingDirectory = cfg.dataDir;
        ReadOnlyPaths = [ cfg.dataDir ];
      };
    };

    # archive exportは一つのSQLite read transactionを使うため、HTTP serverを止めずに一貫した
    # snapshotを取得できる。検証済み世代の作成に成功した後だけ保持処理を実行する。
    systemd.services.marginalis-backup = mkIf (cfg.backupDirectory != null) {
      description = "Create a consistent Marginalis backup";
      environment = {
        RUST_LOG = cfg.logFilter;
        MARGINALIS_DATABASE_URL = "sqlite:${cfg.dataDir}/marginalis.sqlite";
      };
      serviceConfig =
        localServiceConfig
        // {
          Type = "oneshot";
          ExecStart = [
            "${cfg.package}/bin/marginalis backup --directory ${lib.escapeShellArg cfg.backupDirectory}"
            "${cfg.package}/bin/marginalis prune-backups --directory ${lib.escapeShellArg cfg.backupDirectory} --keep ${toString cfg.backupRetention}"
          ];
          WorkingDirectory = cfg.dataDir;
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

    systemd.timers.marginalis-backup = mkIf (cfg.backupDirectory != null) {
      description = "Create and retain verified Marginalis backups daily";
      wantedBy = [ "timers.target" ];
      timerConfig = {
        OnCalendar = "daily";
        Persistent = true;
        Unit = "marginalis-backup.service";
      };
    };

    systemd.services.marginalis-restore-check =
      mkIf (cfg.backupDirectory != null && cfg.restoreCheck.enable)
        {
          description = "Verify the latest Marginalis backup in an isolated database";
          environment.RUST_LOG = cfg.logFilter;
          serviceConfig = localServiceConfig // {
            Type = "oneshot";
            ExecStart = "${cfg.package}/bin/marginalis verify-latest-backup --directory ${lib.escapeShellArg cfg.backupDirectory}";
            WorkingDirectory = cfg.dataDir;
            ReadOnlyPaths = [ cfg.backupDirectory ];
          };
        };

    systemd.timers.marginalis-restore-check =
      mkIf (cfg.backupDirectory != null && cfg.restoreCheck.enable)
        {
          description = "Verify a Marginalis backup on the configured quarterly schedule";
          wantedBy = [ "timers.target" ];
          timerConfig = {
            OnCalendar = cfg.restoreCheck.calendar;
            Persistent = true;
            Unit = "marginalis-restore-check.service";
          };
        };
  };
}
