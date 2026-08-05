import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import { McpAccessSettingsPage } from "../src/routes/McpAccessSettingsPage";

const config = {
  apiBase: "/marginalis/api/v3",
  basePath: "/marginalis",
  path: "/settings/mcp-access",
  search: "",
  styleNonce: "test-style-nonce",
};

const authorization = {
  client_id: "https://client.example.test/oauth/metadata.json",
  display_name: "Research client",
  registration_method: "metadata_document",
  granted_scopes: ["notes:read", "notes:write"],
  scope_ceiling: ["notes:read", "notes:write"],
  scope_ceiling_revision: 0,
  authorized_at_ms: 1_800_000_000_000,
  last_used_at_ms: 1_800_000_001_000,
  active: true,
};

afterEach(() => {
  cleanup();
  document.cookie = "marginalis_csrf=; Max-Age=0; path=/";
  vi.unstubAllGlobals();
});

test("認可済みクライアントのscopeを制限し、確認後に接続を取り消す", async () => {
  document.cookie = "marginalis_csrf=test-csrf; path=/";
  const fetch = vi
    .fn()
    .mockResolvedValueOnce(
      Response.json({
        supported_scopes: ["notes:read", "notes:write", "notes:delete"],
        scopes: ["notes:read", "notes:write"],
        revision: 2,
      }),
    )
    .mockResolvedValueOnce(Response.json([authorization]))
    .mockResolvedValueOnce(
      Response.json({
        ...authorization,
        scope_ceiling: ["notes:read"],
        scope_ceiling_revision: 1,
      }),
    )
    .mockResolvedValueOnce(new Response(null, { status: 204 }));
  vi.stubGlobal("fetch", fetch);

  render(<McpAccessSettingsPage config={config} />);

  const heading = await screen.findByRole("heading", {
    name: "Research client",
  });
  const card = heading.closest("article");
  expect(card).not.toBeNull();
  const client = within(card!);
  expect(client.getByText("Client ID Metadata Document")).toBeInTheDocument();
  expect(client.getByText("有効")).toBeInTheDocument();
  fireEvent.click(client.getByLabelText(/notes:write/));
  fireEvent.click(client.getByRole("button", { name: "上限を保存" }));

  await waitFor(() => expect(fetch).toHaveBeenCalledTimes(3));
  expect(fetch).toHaveBeenNthCalledWith(
    3,
    "/marginalis/api/v3/mcp-authorizations/https%3A%2F%2Fclient.example.test%2Foauth%2Fmetadata.json/scope-ceiling",
    expect.objectContaining({
      method: "PUT",
      headers: expect.objectContaining({ "x-csrf-token": "test-csrf" }),
      body: JSON.stringify({ scopes: ["notes:read"], revision: 0 }),
    }),
  );

  fireEvent.click(client.getByRole("button", { name: "接続を取り消す" }));
  expect(screen.getByRole("dialog")).toHaveTextContent(
    "access tokenとrefresh tokenを直ちに失効",
  );
  fireEvent.click(
    within(screen.getByRole("dialog")).getByRole("button", {
      name: "接続を取り消す",
    }),
  );

  await waitFor(() => expect(fetch).toHaveBeenCalledTimes(4));
  expect(fetch).toHaveBeenLastCalledWith(
    "/marginalis/api/v3/mcp-authorizations/https%3A%2F%2Fclient.example.test%2Foauth%2Fmetadata.json",
    expect.objectContaining({ method: "DELETE" }),
  );
  expect(await screen.findByText("無効")).toBeInTheDocument();
});
