import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import { WebhookSettingsPage } from "../src/routes/WebhookSettingsPage";

const config = {
  apiBase: "/marginalis/api/v3",
  basePath: "/marginalis",
  path: "/settings/webhooks",
  search: "",
  styleNonce: "test-style-nonce",
};

const subscription = {
  subscription_id: "0197c9bc-0000-7000-8000-000000000001",
  url: "https://receiver.example.com/hooks",
  event_kinds: ["note.created"],
  state: "pending_challenge",
  disabled_reason: null,
  created_at_ms: 1_800_000_000_000,
  updated_at_ms: 1_800_000_000_000,
  revision: 1,
  last_attempted_at_ms: null,
  last_failure: null,
  next_attempt_at_ms: null,
  pending_count: 0,
};

afterEach(() => {
  cleanup();
  document.cookie = "marginalis_csrf=; Max-Age=0; path=/";
  vi.unstubAllGlobals();
});

test("Webhookを登録するとsecretが1回だけ表示され、検証で有効になる", async () => {
  document.cookie = "marginalis_csrf=test-csrf; path=/";
  const fetch = vi
    .fn()
    .mockResolvedValueOnce(Response.json([]))
    .mockResolvedValueOnce(
      Response.json(
        { subscription, secret: "secret-value-1" },
        { status: 201 },
      ),
    )
    .mockResolvedValueOnce(Response.json([subscription]))
    .mockResolvedValueOnce(Response.json({ verified: true, failure: null }))
    .mockResolvedValueOnce(
      Response.json([{ ...subscription, state: "active", revision: 2 }]),
    );
  vi.stubGlobal("fetch", fetch);

  render(<WebhookSettingsPage config={config} />);

  expect(
    await screen.findByText("登録済みのWebhookはありません。"),
  ).toBeInTheDocument();

  fireEvent.change(screen.getByLabelText(/送信先URL/), {
    target: { value: "https://receiver.example.com/hooks" },
  });
  fireEvent.click(screen.getByLabelText(/note\.created/));
  fireEvent.click(screen.getByRole("button", { name: "Webhookを登録" }));

  // secretは登録応答からそのまま表示され、再取得はできない。
  expect(await screen.findByText("secret-value-1")).toBeInTheDocument();
  expect(fetch).toHaveBeenNthCalledWith(
    2,
    "/marginalis/api/v3/webhooks",
    expect.objectContaining({
      method: "POST",
      headers: expect.objectContaining({ "x-csrf-token": "test-csrf" }),
      body: JSON.stringify({
        url: "https://receiver.example.com/hooks",
        event_kinds: ["note.created"],
      }),
    }),
  );

  const verifyButton = await screen.findByRole("button", {
    name: "送信先を確認して有効化",
  });
  fireEvent.click(verifyButton);

  await waitFor(() => expect(fetch).toHaveBeenCalledTimes(5));
  expect(fetch).toHaveBeenNthCalledWith(
    4,
    `/marginalis/api/v3/webhooks/${subscription.subscription_id}/verify`,
    expect.objectContaining({ method: "POST" }),
  );
  expect(
    await screen.findByText("送信先を確認し、Webhookを有効にしました。"),
  ).toBeInTheDocument();
  await waitFor(() => expect(screen.getByText("有効")).toBeInTheDocument());
});

test("停止中のWebhookは理由と再試行の操作を表示する", async () => {
  const disabled = {
    ...subscription,
    state: "disabled",
    disabled_reason: "delivery_exhausted",
    last_attempted_at_ms: 1_800_000_100_000,
    last_failure: "timed_out",
    next_attempt_at_ms: 1_800_003_700_000,
    pending_count: 3,
  };
  const fetch = vi.fn().mockResolvedValueOnce(Response.json([disabled]));
  vi.stubGlobal("fetch", fetch);

  render(<WebhookSettingsPage config={config} />);

  expect(
    await screen.findByText(/配送の再試行が上限に達したため停止しています/),
  ).toBeInTheDocument();
  expect(screen.getByText("停止中")).toBeInTheDocument();
  expect(screen.getByText("3件")).toBeInTheDocument();
  expect(
    screen.getByText("送信が時間内に完了しませんでした"),
  ).toBeInTheDocument();
  expect(
    screen.getByRole("button", { name: "失敗した配送を再試行" }),
  ).toBeInTheDocument();
  expect(
    screen.getByRole("button", { name: "失敗した配送を破棄" }),
  ).toBeInTheDocument();
});
