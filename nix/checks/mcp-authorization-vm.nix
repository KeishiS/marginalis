# 内蔵Authorization Serverのdiscovery文書と、無効tokenの拒否をVM上で確かめる。
{ pkgs, self }:
pkgs.testers.nixosTest {
  name = "marginalis-mcp-authorization";
  nodes.app = {
    imports = [ self.nixosModules.default ];
    system.stateVersion = "25.11";
    environment.etc."marginalis-test/oidc-client-secret".text = "test-only-secret";
    environment.systemPackages = [
      pkgs.coreutils
      pkgs.curl
      pkgs.jq
    ];
    services.marginalis = {
      enable = true;
      baseUrl = "https://marginalis.example.test";
      oidc = {
        issuerUrl = "https://id.example.test";
        clientId = "marginalis";
        clientSecretFile = "/etc/marginalis-test/oidc-client-secret";
      };
      mcp = {
        enable = true;
      };
    };
  };

  testScript = ''
    app.start()
    app.wait_for_unit("marginalis.service")
    app.wait_until_succeeds(
      "curl -fsS http://127.0.0.1:3000/api/v3/health | jq -e '.status == \"ok\"'"
    )
    app.succeed(
      "curl -fsS http://127.0.0.1:3000/.well-known/oauth-protected-resource/mcp "
      + "| jq -e '.resource == \"https://marginalis.example.test/mcp\" "
      + "and .authorization_servers == [\"https://marginalis.example.test/\"]'"
    )
    app.succeed(
      "curl -fsS http://127.0.0.1:3000/.well-known/oauth-authorization-server "
      + "| jq -e '.issuer == \"https://marginalis.example.test/\" "
      + "and .authorization_endpoint == \"https://marginalis.example.test/oauth/authorize\" "
      + "and .token_endpoint == \"https://marginalis.example.test/oauth/token\" "
      + "and .revocation_endpoint == \"https://marginalis.example.test/oauth/revoke\" "
      + "and .code_challenge_methods_supported == [\"S256\"]'"
    )
    app.fail(
      "curl -fsS -H 'Authorization: Bearer invalid-token' "
      + "-H 'Accept: application/json, text/event-stream' "
      + "-H 'Content-Type: application/json' "
      + "--data '{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/list\"}' "
      + "http://127.0.0.1:3000/mcp"
    )
    app.succeed("systemctl stop marginalis.service")
  '';
}
