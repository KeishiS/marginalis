const { test, expect } = require("./fixtures/browser-diagnostics");

const baseUrl = "https://marginalis.example.test/marginalis";

test("Kanidm login works through the subpath", async ({ page, context }) => {
  await page.goto(`${baseUrl}/auth/oidc/login?next=%2Fmarginalis%2F`);
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
  await expect(page).toHaveURL("https://id.example.test:8443/ui/oauth2/resume");
  await page.getByRole("button", { name: "Proceed", exact: true }).click();

  await expect(page).toHaveURL(`${baseUrl}/`);
  await expect(page.locator("body")).toContainText("Marginalis");
});
