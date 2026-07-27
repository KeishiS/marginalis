const { createHash } = require("node:crypto");
const { test, expect } = require("@playwright/test");

const baseUrl = "https://marginalis.example.test/marginalis";
const callbackUrl = "http://127.0.0.1:49152/callback";
const resourceUrl = `${baseUrl}/mcp`;
const verifier =
  "browser-pkce-verifier-with-more-than-forty-three-characters";

function challenge(value) {
  return createHash("sha256").update(value).digest("base64url");
}

async function formValues(page) {
  return Object.fromEntries(
    await page.locator("form input").evaluateAll((inputs) =>
      inputs.map((input) => [input.name, input.value]),
    ),
  );
}

async function tokenRequest(request, values) {
  return request.post(`${baseUrl}/oauth/token`, {
    form: values,
    failOnStatusCode: false,
  });
}

test("Kanidm login and MCP OAuth lifecycle work through the subpath", async ({
  page,
  context,
}) => {
  await page.goto(
    `${baseUrl}/auth/oidc/login?next=%2Fmarginalis%2F`,
  );
  await expect(page).toHaveURL(/^https:\/\/id\.example\.test:8443\//);

  const cookies = await context.cookies();
  expect(
    cookies.some(
      (cookie) =>
        cookie.domain === "marginalis.example.test" &&
        cookie.path === "/marginalis" &&
        cookie.secure,
    ),
  ).toBe(true);

  await page
    .getByRole("textbox", { name: "Username", exact: true })
    .fill("idm_admin");
  await page
    .getByRole("textbox", { name: "Username", exact: true })
    .press("Enter");
  await page.getByLabel(/password/i).fill("test-idm-admin-password");
  await page.getByLabel(/password/i).press("Enter");
  await expect(page).toHaveURL(
    "https://id.example.test:8443/ui/oauth2/resume",
  );
  await page.getByRole("button", { name: "Proceed", exact: true }).click();

  await expect(page).toHaveURL(`${baseUrl}/`);
  await expect(page.locator("body")).toContainText("Marginalis");

  const registration = await context.request.post(`${baseUrl}/oauth/register`, {
    data: {
      client_name: "Browser regression client",
      redirect_uris: [callbackUrl],
    },
    failOnStatusCode: false,
  });
  expect(registration.status()).toBe(201);
  const clientId = (await registration.json()).client_id;
  expect(clientId).toBeTruthy();

  const authorization = new URL(`${baseUrl}/oauth/authorize`);
  authorization.search = new URLSearchParams({
    response_type: "code",
    client_id: clientId,
    redirect_uri: callbackUrl,
    resource: resourceUrl,
    scope: "notes:read notes:write notes:delete",
    code_challenge: challenge(verifier),
    code_challenge_method: "S256",
    state: "browser-client-state",
  });
  await page.goto(authorization.toString());
  await expect(page.getByRole("heading")).toContainText("Authorize");
  const consent = await formValues(page);

  const rejected = await context.request.post(
    `${baseUrl}/oauth/authorize/consent`,
    {
      form: { ...consent, decision: "approve" },
      headers: {
        Origin: "https://evil.example",
        "Sec-Fetch-Site": "cross-site",
      },
      failOnStatusCode: false,
      maxRedirects: 0,
    },
  );
  expect(rejected.status()).toBe(403);

  const approved = await context.request.post(
    `${baseUrl}/oauth/authorize/consent`,
    {
      form: { ...consent, decision: "approve" },
      headers: {
        Origin: "https://marginalis.example.test",
        "Sec-Fetch-Site": "same-origin",
      },
      failOnStatusCode: false,
      maxRedirects: 0,
    },
  );
  expect(approved.status()).toBe(303);
  const redirect = new URL(approved.headers().location);
  expect(redirect.origin + redirect.pathname).toBe(callbackUrl);
  expect(redirect.searchParams.get("state")).toBe("browser-client-state");
  const code = redirect.searchParams.get("code");
  expect(code).toBeTruthy();

  const issued = await tokenRequest(context.request, {
    grant_type: "authorization_code",
    code,
    client_id: clientId,
    redirect_uri: callbackUrl,
    resource: resourceUrl,
    code_verifier: verifier,
  });
  expect(issued.status()).toBe(200);
  const issuedTokens = await issued.json();

  const refreshed = await tokenRequest(context.request, {
    grant_type: "refresh_token",
    client_id: clientId,
    resource: resourceUrl,
    refresh_token: issuedTokens.refresh_token,
  });
  expect(refreshed.status()).toBe(200);
  const tokens = await refreshed.json();
  expect(tokens.access_token).not.toBe(issuedTokens.access_token);
  expect(tokens.refresh_token).not.toBe(issuedTokens.refresh_token);

  const initialize = await context.request.post(resourceUrl, {
    data: {
      jsonrpc: "2.0",
      id: 1,
      method: "initialize",
      params: {
        protocolVersion: "2025-03-26",
        capabilities: {},
        clientInfo: { name: "browser-regression-client", version: "1" },
      },
    },
    headers: {
      Accept: "application/json, text/event-stream",
      Authorization: `Bearer ${tokens.access_token}`,
    },
    failOnStatusCode: false,
  });
  expect(initialize.status()).toBe(200);
  const initialized = await initialize.json();
  expect(initialized.result.protocolVersion).toBe("2025-03-26");

  const reused = await tokenRequest(context.request, {
    grant_type: "refresh_token",
    client_id: clientId,
    resource: resourceUrl,
    refresh_token: issuedTokens.refresh_token,
  });
  expect(reused.status()).toBe(400);

  const afterReplay = await context.request.post(resourceUrl, {
    data: { jsonrpc: "2.0", id: 2, method: "ping" },
    headers: {
      Accept: "application/json, text/event-stream",
      Authorization: `Bearer ${tokens.access_token}`,
    },
    failOnStatusCode: false,
  });
  expect(afterReplay.status()).toBe(401);

  const csrf = (await context.cookies()).find(
    (cookie) =>
      cookie.name === "marginalis_csrf" &&
      cookie.domain === "marginalis.example.test",
  );
  expect(csrf).toBeTruthy();
  const revoked = await context.request.delete(
    `${baseUrl}/api/v2/mcp-authorizations/${encodeURIComponent(clientId)}`,
    {
      headers: {
        Origin: "https://marginalis.example.test",
        "Sec-Fetch-Site": "same-origin",
        "X-CSRF-Token": csrf.value,
      },
      failOnStatusCode: false,
    },
  );
  expect(revoked.status()).toBe(204);
});
