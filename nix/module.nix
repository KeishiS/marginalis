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

    baseUrl = mkOption {
      type = types.nullOr types.str;
      default = null;
      example = "https://marginalis.example.test";
      description = "Public HTTPS Base URL, including any reverse-proxy subpath.";
    };

    dataDir = mkOption {
      type = types.str;
      default = "/var/lib/marginalis";
      description = "Directory holding the AsciiDoc source of record and SQLite database.";
    };

    databaseUrl = mkOption {
      type = types.nullOr types.str;
      default = null;
      description = "SQLite connection URL. Defaults to a database below dataDir.";
    };

    initialRegistrationPolicy = mkOption {
      type = types.enum [
        "open"
        "approval"
      ];
      default = "approval";
      description = "Registration policy written only when Marginalis creates a new database. Later root API changes are preserved.";
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

    initialRootPasswordFile = mkOption {
      type = types.nullOr types.str;
      default = null;
      example = "/run/secrets/marginalis-root-password";
      description = "Optional runtime path to the one-time root password. Required only while the database has no root account.";
    };

    mcp = {
      enable = mkOption {
        type = types.bool;
        default = false;
        description = "Whether to expose the OAuth-protected MCP endpoint and authorization server.";
      };

      clientMetadataAllowedHosts = mkOption {
        type = types.listOf types.str;
        default = [ ];
        example = [ "clients.example.org" ];
        description = "HTTPS hosts from which MCP Client ID Metadata Documents may be fetched. Keeping this explicit prevents the authorization endpoint from becoming an SSRF primitive.";
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
    ];

    users.groups.marginalis = { };
    users.users.marginalis = {
      isSystemUser = true;
      group = "marginalis";
    };

    systemd.tmpfiles.rules = [ "d ${cfg.dataDir} 0750 marginalis marginalis -" ];

    networking.firewall.allowedTCPPorts = optionals cfg.openFirewall [ listenPort ];

    systemd.services.marginalis = {
      description = "Marginalis research-note server";
      wantedBy = [ "multi-user.target" ];
      after = [ "network-online.target" ];
      wants = [ "network-online.target" ];
      environment = {
        MARGINALIS_BASE_URL = cfg.baseUrl;
        MARGINALIS_LISTEN_ADDR = cfg.listenAddress;
        MARGINALIS_DATA_DIR = cfg.dataDir;
        MARGINALIS_DATABASE_URL =
          if cfg.databaseUrl == null then "sqlite:${cfg.dataDir}/marginalis.sqlite" else cfg.databaseUrl;
        MARGINALIS_INITIAL_REGISTRATION_POLICY = cfg.initialRegistrationPolicy;
        OIDC_ISSUER_URL = cfg.oidc.issuerUrl;
        OIDC_CLIENT_ID = cfg.oidc.clientId;
        OIDC_CLIENT_SECRET_FILE = "%d/oidc-client-secret";
        MARGINALIS_MCP_ENABLE = if cfg.mcp.enable then "true" else "false";
        MARGINALIS_MCP_CLIENT_METADATA_ALLOWED_HOSTS = lib.concatStringsSep "," cfg.mcp.clientMetadataAllowedHosts;
      }
      // optionalAttrs (cfg.initialRootPasswordFile != null) {
        ROOT_PASSWORD_FILE = "%d/root-password";
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
        ]
        ++ optionals (cfg.initialRootPasswordFile != null) [
          "root-password:${cfg.initialRootPasswordFile}"
        ];
        NoNewPrivileges = true;
        PrivateTmp = true;
        ProtectHome = true;
        ProtectSystem = "strict";
        ReadWritePaths = [ cfg.dataDir ];
      }
      // optionalAttrs (cfg.dataDir == "/var/lib/marginalis") {
        # 既定の永続領域はservice開始前にsystemd自身が作成する。手動削除後も
        # ReadWritePathsのmount namespace構築より先に復元される。
        StateDirectory = "marginalis";
        StateDirectoryMode = "0750";
      };
    };

    # 正本からの投影再構築はHTTP serverと同時実行しない。systemdのcredential注入を再利用するため、
    # 手動の環境変数指定ではなくこのoneshot unitを運用入口とする。
    systemd.services.marginalis-rebuild-projections = {
      description = "Rebuild Marginalis SQLite projections from canonical sources";
      conflicts = [ "marginalis.service" ];
      environment = {
        MARGINALIS_BASE_URL = cfg.baseUrl;
        MARGINALIS_LISTEN_ADDR = cfg.listenAddress;
        MARGINALIS_DATA_DIR = cfg.dataDir;
        MARGINALIS_DATABASE_URL =
          if cfg.databaseUrl == null then "sqlite:${cfg.dataDir}/marginalis.sqlite" else cfg.databaseUrl;
        MARGINALIS_INITIAL_REGISTRATION_POLICY = cfg.initialRegistrationPolicy;
        OIDC_ISSUER_URL = cfg.oidc.issuerUrl;
        OIDC_CLIENT_ID = cfg.oidc.clientId;
        OIDC_CLIENT_SECRET_FILE = "%d/oidc-client-secret";
        MARGINALIS_MCP_ENABLE = if cfg.mcp.enable then "true" else "false";
        MARGINALIS_MCP_CLIENT_METADATA_ALLOWED_HOSTS = lib.concatStringsSep "," cfg.mcp.clientMetadataAllowedHosts;
      }
      // optionalAttrs (cfg.initialRootPasswordFile != null) {
        ROOT_PASSWORD_FILE = "%d/root-password";
      };
      serviceConfig = {
        Type = "oneshot";
        ExecStart = "${cfg.package}/bin/marginalis rebuild-projections";
        User = "marginalis";
        Group = "marginalis";
        WorkingDirectory = cfg.dataDir;
        LoadCredential = [
          "oidc-client-secret:${cfg.oidc.clientSecretFile}"
        ]
        ++ optionals (cfg.initialRootPasswordFile != null) [
          "root-password:${cfg.initialRootPasswordFile}"
        ];
        NoNewPrivileges = true;
        PrivateTmp = true;
        ProtectHome = true;
        ProtectSystem = "strict";
        ReadWritePaths = [ cfg.dataDir ];
      }
      // optionalAttrs (cfg.dataDir == "/var/lib/marginalis") {
        StateDirectory = "marginalis";
        StateDirectoryMode = "0750";
      };
    };
  };
}
