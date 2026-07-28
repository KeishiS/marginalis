import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import { AccessControl } from "../src/AccessControl";

const NOTE_ID = "0197c9bc-0000-7000-8000-000000000001";

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

test("subjectと権限を指定して共有設定を保存する", async () => {
  const fetchMock = vi
    .fn()
    .mockResolvedValueOnce(
      new Response(JSON.stringify({ entries: [] }), {
        status: 200,
        headers: { "content-type": "application/json" },
      }),
    )
    .mockResolvedValueOnce(
      new Response(
        JSON.stringify({
          note_id: NOTE_ID,
          title: "共有",
          body: "本文",
          tags: [],
          created_at_ms: 1,
          updated_at_ms: 2,
          revision: 2,
        }),
        { status: 200, headers: { "content-type": "application/json" } },
      ),
    );
  vi.stubGlobal("fetch", fetchMock);

  render(
    <AccessControl
      apiBase="/api/v2"
      noteId={NOTE_ID}
      revision={1}
      onRevision={vi.fn()}
    />,
  );
  fireEvent.change(
    await screen.findByRole("textbox", { name: "利用者subject" }),
    {
      target: { value: "reader-subject" },
    },
  );
  fireEvent.change(screen.getByRole("combobox", { name: "権限" }), {
    target: { value: "edit" },
  });
  fireEvent.click(screen.getByRole("button", { name: "共有先を追加" }));
  fireEvent.click(screen.getByRole("button", { name: "共有設定を保存" }));

  await screen.findByText("共有設定を保存しました。");
  const request = fetchMock.mock.calls[1]?.[1] as RequestInit;
  expect(JSON.parse(String(request.body))).toEqual({
    entries: [{ subject: "reader-subject", permission: "edit" }],
    expected_revision: 1,
  });
});

test("revision競合時は再読み込みを案内する", async () => {
  vi.stubGlobal(
    "fetch",
    vi
      .fn()
      .mockResolvedValueOnce(
        new Response(JSON.stringify({ entries: [] }), { status: 200 }),
      )
      .mockResolvedValueOnce(
        new Response(
          JSON.stringify({ code: "conflict", message: "conflict" }),
          {
            status: 409,
          },
        ),
      ),
  );
  render(
    <AccessControl
      apiBase="/api/v2"
      noteId={NOTE_ID}
      revision={1}
      onRevision={vi.fn()}
    />,
  );
  await waitFor(() =>
    expect(
      screen.getByRole("button", { name: "共有設定を保存" }),
    ).toBeEnabled(),
  );
  fireEvent.click(screen.getByRole("button", { name: "共有設定を保存" }));
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "画面を再読み込み",
  );
});
