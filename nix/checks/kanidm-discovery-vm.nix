# 実 Kanidm 1.10、private CA、nginx TLS、subpathを通して、Web用OIDC Discoveryと
# browser login開始を確認する。対話loginとgroup変更は手動受入で扱う。
{ pkgs, self }:
let
  kanidmDiscoveryCerts =
    pkgs.runCommand "marginalis-kanidm-discovery-certs"
      {
        nativeBuildInputs = [ pkgs.openssl ];
      }
      ''
        # Nix storeに保存されたfixtureを日をまたいで再利用しても失効しないようにする。
        openssl req -x509 -newkey rsa:2048 -nodes -days 36500 \
          -subj '/CN=Marginalis Kanidm Test CA' \
          -addext 'basicConstraints=critical,CA:TRUE' \
          -addext 'keyUsage=critical,keyCertSign' \
          -keyout ca-key.pem -out ca-cert.pem
        openssl req -newkey rsa:2048 -nodes \
          -subj '/CN=id.example.test' \
          -addext 'subjectAltName=DNS:id.example.test' \
          -keyout $out-key.pem -out request.pem
        openssl x509 -req -in request.pem -CA ca-cert.pem -CAkey ca-key.pem \
          -CAcreateserial -days 36500 -out $out-cert.pem \
          -extfile <(printf 'basicConstraints=critical,CA:FALSE\nkeyUsage=critical,digitalSignature,keyEncipherment\nsubjectAltName=DNS:id.example.test')
        mkdir -p $out
        mv $out-key.pem $out/id-key.pem
        mv $out-cert.pem $out/id-cert.pem
        openssl req -newkey rsa:2048 -nodes \
          -subj '/CN=marginalis.example.test' \
          -addext 'subjectAltName=DNS:marginalis.example.test' \
          -keyout $out/app-key.pem -out app-request.pem
        openssl x509 -req -in app-request.pem -CA ca-cert.pem -CAkey ca-key.pem \
          -CAserial ca-cert.srl -days 36500 -out $out/app-cert.pem \
          -extfile <(printf 'basicConstraints=critical,CA:FALSE\nkeyUsage=critical,digitalSignature,keyEncipherment\nsubjectAltName=DNS:marginalis.example.test')
        mv ca-cert.pem $out/ca.pem
      '';
in
pkgs.testers.nixosTest {
  name = "marginalis-kanidm-discovery";
  nodes.idp = {
    environment.systemPackages = [ pkgs.kanidm_1_10 ];
    services.kanidm = {
      package = pkgs.kanidmWithSecretProvisioning_1_10;
      server = {
        enable = true;
        settings = {
          origin = "https://id.example.test:8443";
          domain = "id.example.test";
          bindaddress = "0.0.0.0:8443";
          tls_chain = "${kanidmDiscoveryCerts}/id-cert.pem";
          tls_key = "${kanidmDiscoveryCerts}/id-key.pem";
        };
      };
      provision = {
        enable = true;
        instanceUrl = "https://localhost:8443";
        acceptInvalidCerts = true;
        idmAdminPasswordFile = pkgs.writeText "marginalis-test-idm-admin-password" "test-idm-admin-password";
        groups.server-users = { };
        systems.oauth2.marginalis = {
          displayName = "Marginalis test client";
          originUrl = "https://marginalis.example.test/marginalis/auth/oidc/callback";
          originLanding = "https://marginalis.example.test/marginalis";
          basicSecretFile = pkgs.writeText "marginalis-test-oidc-secret" "test-only-secret";
          scopeMaps.server-users = [
            "openid"
            "profile"
            "email"
            "groups_name"
          ];
          claimMaps.groups = {
            joinType = "array";
            valuesByGroup.server-users = [ "server-users" ];
          };
        };
      };
    };
    networking.firewall.allowedTCPPorts = [ 8443 ];
  };
  nodes.app =
    { nodes, ... }:
    {
      imports = [ self.nixosModules.default ];
      system.stateVersion = "25.11";
      # NixOS test driverのeth0 DHCPアドレスは各VMで重複するため、隔離VLANの
      # IdPアドレスを使う。idpは二番目に起動するVMなので192.168.1.2となる。
      networking.hosts."192.168.1.2" = [ "id.example.test" ];
      networking.hosts."127.0.0.1" = [ "marginalis.example.test" ];
      security.pki.certificateFiles = [ "${kanidmDiscoveryCerts}/ca.pem" ];
      environment.etc."marginalis-test/oidc-client-secret".text = "test-only-secret";
      environment.systemPackages = [
        pkgs.kanidm_1_10
        pkgs.playwright-test
        pkgs.playwright-driver.browsers
        pkgs.ripgrep
        pkgs.sqlite
      ];
      services.nginx = {
        enable = true;
        virtualHosts."marginalis.example.test" = {
          forceSSL = true;
          sslCertificate = "${kanidmDiscoveryCerts}/app-cert.pem";
          sslCertificateKey = "${kanidmDiscoveryCerts}/app-key.pem";
          locations."/marginalis/".proxyPass = "http://127.0.0.1:3000/";
        };
      };
      services.marginalis = {
        enable = true;
        baseUrl = "https://marginalis.example.test/marginalis";
        oidc = {
          issuerUrl = "https://id.example.test:8443/oauth2/openid/marginalis";
          clientId = "marginalis";
          clientSecretFile = "/etc/marginalis-test/oidc-client-secret";
          caCertificateFile = "${kanidmDiscoveryCerts}/ca.pem";
        };
      };
    };
  testScript = ''
    idp.start()
    idp.wait_for_unit("kanidm.service")
    idp.wait_until_succeeds(
      "curl --insecure --resolve id.example.test:8443:127.0.0.1 -Lsf https://id.example.test:8443 | grep Kanidm"
    )
    idp.succeed(
      "KANIDM_PASSWORD=test-idm-admin-password "
      + "kanidm login --accept-invalid-certs -H https://localhost:8443 -D idm_admin"
    )
    idp.succeed(
      "kanidm group add-members --accept-invalid-certs "
      + "-H https://localhost:8443 -D idm_admin server-users idm_admin"
    )
    app.start()
    app.wait_for_unit("marginalis.service")
    app.wait_for_unit("nginx.service")
    app.wait_until_succeeds("curl -fsS http://127.0.0.1:3000/api/v3/health | grep -q '\"api_version\":\"v3\"'")
    app.wait_until_succeeds(
      "systemctl status marginalis.service --no-pager --full | "
      + "grep -F 'http.request.completed' | grep -F 'outcome=' | grep -Fq success"
    )
    app.succeed(
      "headers=$(mktemp); "
      + "curl --cacert ${kanidmDiscoveryCerts}/ca.pem -sS -D \"$headers\" -o /dev/null "
      + "'https://marginalis.example.test/marginalis/auth/oidc/login?next=%2Fmarginalis%2F'; "
      + "grep -qi '^set-cookie: marginalis_return_to=.*Path=/marginalis.*Secure' \"$headers\"; "
      + "grep -qi '^location: https://id.example.test:8443/' \"$headers\""
    )
    app.succeed(
      "test $(curl --cacert ${kanidmDiscoveryCerts}/ca.pem -sS -o /dev/null -w '%{http_code}' "
      + "'https://marginalis.example.test/marginalis/auth/oidc/callback?code=invalid&state=invalid') = 401"
    )
    app.wait_until_succeeds(
      "systemctl status marginalis.service --no-pager --full | "
      + "grep -F 'http.request.completed' | grep -F 'status=401' | "
      + "grep -F 'outcome=' | grep -Fq rejected"
    )
    app.succeed(
      "sqlite3 /var/lib/marginalis/marginalis.sqlite < ${./fixtures/web-session-seed.sql}"
    )
    app.succeed(
      "mkdir -p /tmp/browser; "
      + "cp -r ${../../tests/browser}/. /tmp/browser/; "
      + "cd /tmp/browser; "
      + "set +e; NODE_EXTRA_CA_CERTS=${kanidmDiscoveryCerts}/ca.pem "
      + "playwright test --config playwright.vm.config.js "
      + ">/tmp/playwright-raw.log 2>&1; status=$?; set -e; "
      + "bash ${../../.github/scripts/protocol-artifact.sh} sanitize "
      + "/tmp/playwright-raw.log /tmp/playwright.log; "
      + "bash ${../../.github/scripts/protocol-artifact.sh} check /tmp/playwright.log; "
      + "cat /tmp/playwright.log; exit $status"
    )
    app.succeed("journalctl -u marginalis.service | grep -q 'Marginalis server listening'")
    app.succeed("! journalctl -u marginalis.service | grep -q 'OIDC discovery is unavailable'")
  '';
}
