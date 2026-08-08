import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import { MathMacroSettingsPage } from "../src/routes/MathMacroSettingsPage";

const CONFIG = {
  apiBase: "/api/v3",
  basePath: "/",
  path: "/settings/math-macros",
  search: "",
  styleNonce: "test-nonce",
};

afterEach(() => {
  cleanup();
  document.cookie = "marginalis_csrf=; Max-Age=0; path=/";
  vi.unstubAllGlobals();
});

function settingsResponse(macros: unknown[] = [], revision = 0) {
  return new Response(JSON.stringify({ macros, revision }), {
    status: 200,
    headers: { "content-type": "application/json" },
  });
}

test("現在のrevisionと編集内容を保存し、保存後のrevisionを採用する", async () => {
  document.cookie = "marginalis_csrf=test-csrf; path=/";
  const fetchMock = vi
    .fn()
    .mockResolvedValueOnce(settingsResponse([], 2))
    .mockResolvedValueOnce(
      settingsResponse(
        [{ name: "argmax", replacement: "result", argument_count: 0 }],
        3,
      ),
    );
  vi.stubGlobal("fetch", fetchMock);
  render(<MathMacroSettingsPage config={CONFIG} />);

  fireEvent.click(await screen.findByRole("button", { name: /argmax/ }));
  fireEvent.change(screen.getByLabelText("置換内容"), {
    target: { value: "result" },
  });
  fireEvent.click(screen.getByRole("button", { name: "保存" }));

  await screen.findByText("数式マクロを保存しました。");
  const request = fetchMock.mock.calls[1][1] as RequestInit;
  expect(request.method).toBe("PUT");
  expect(JSON.parse(String(request.body))).toEqual({
    macros: [{ name: "argmax", replacement: "result", argument_count: 0 }],
    revision: 2,
  });
});

test("revision競合を入力不正や通信失敗と区別して案内する", async () => {
  const fetchMock = vi
    .fn()
    .mockResolvedValueOnce(settingsResponse([], 2))
    .mockResolvedValueOnce(
      new Response(
        JSON.stringify({ code: "conflict", message: "競合しました。" }),
        { status: 409, headers: { "content-type": "application/json" } },
      ),
    );
  vi.stubGlobal("fetch", fetchMock);
  render(<MathMacroSettingsPage config={CONFIG} />);

  fireEvent.click(await screen.findByRole("button", { name: /argmax/ }));
  fireEvent.click(screen.getByRole("button", { name: "保存" }));

  expect(await screen.findByRole("alert")).toHaveTextContent(
    "別の画面で数式マクロが更新されています",
  );
  expect(screen.getByLabelText("コマンド名")).toHaveValue("argmax");
});

test("重複名と全体の大きさを保存前に案内する", async () => {
  const largeMacros = Array.from({ length: 33 }, (_, index) => ({
    name: `m${String.fromCharCode(65 + (index % 26))}${String.fromCharCode(65 + Math.floor(index / 26))}`,
    replacement: "あ".repeat(512),
    argument_count: 0,
  }));
  const fetchMock = vi.fn().mockResolvedValue(settingsResponse(largeMacros, 1));
  vi.stubGlobal("fetch", fetchMock);
  const { unmount } = render(<MathMacroSettingsPage config={CONFIG} />);

  expect(await screen.findByRole("alert")).toHaveTextContent("16 KiB以下");
  expect(screen.getByRole("button", { name: "保存" })).toBeDisabled();
  expect(screen.getByText(/33 \/ 64件/)).toBeInTheDocument();

  unmount();
  vi.stubGlobal(
    "fetch",
    vi.fn().mockResolvedValue(
      settingsResponse(
        [
          { name: "same", replacement: "a", argument_count: 0 },
          { name: "same", replacement: "b", argument_count: 0 },
        ],
        1,
      ),
    ),
  );
  render(<MathMacroSettingsPage config={CONFIG} />);
  await waitFor(() =>
    expect(screen.getByRole("alert")).toHaveTextContent("重複しています"),
  );
});
