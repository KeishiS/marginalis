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

test("issuer、subject、権限を指定して共有設定を保存する", async () => {
  const fetchMock = vi
    .fn()
    .mockResolvedValueOnce(
      new Response(JSON.stringify({ entries: [] }), {
        status: 200,
        headers: {
          "content-type": "application/json",
          etag: '"rev-1"',
        },
      }),
    )
    .mockResolvedValueOnce(
      new Response(
        JSON.stringify({
          note_id: NOTE_ID,
          title: "共有",
          source: "= 共有\n\n本文",
          tags: [],
          created_at_ms: 1,
          updated_at_ms: 2,
          revision: 2,
          created_via: "web",
          review_status: "pending",
          reviewed_revision: null,
          reviewed_at_ms: null,
        }),
        { status: 200, headers: { "content-type": "application/json" } },
      ),
    );
  vi.stubGlobal("fetch", fetchMock);

  render(
    <AccessControl
      apiBase="/api/v3"
      noteId={NOTE_ID}
      revision={1}
      onRevision={vi.fn()}
    />,
  );
  fireEvent.change(
    await screen.findByRole("textbox", { name: "OIDC issuer" }),
    {
      target: { value: "https://id.example.test" },
    },
  );
  fireEvent.change(screen.getByRole("textbox", { name: "利用者subject" }), {
    target: { value: "reader-subject" },
  });
  fireEvent.change(screen.getByRole("combobox", { name: "権限" }), {
    target: { value: "edit" },
  });
  fireEvent.click(screen.getByRole("button", { name: "共有先を追加" }));
  fireEvent.click(screen.getByRole("button", { name: "共有設定を保存" }));

  await screen.findByText("共有設定を保存しました。");
  const request = fetchMock.mock.calls[1]?.[1] as RequestInit;
  expect(JSON.parse(String(request.body))).toEqual({
    entries: [
      {
        issuer: "https://id.example.test",
        subject: "reader-subject",
        permission: "edit",
      },
    ],
  });
  expect(request.headers).toEqual(
    expect.objectContaining({ "if-match": '"rev-1"' }),
  );
});

test("revision競合時は再読み込みを案内する", async () => {
  vi.stubGlobal(
    "fetch",
    vi
      .fn()
      .mockResolvedValueOnce(
        new Response(JSON.stringify({ entries: [] }), {
          status: 200,
          headers: { etag: '"rev-1"' },
        }),
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
      apiBase="/api/v3"
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

test("保存中は同じ更新番号の要求を重ねて送らない", async () => {
  let completeSave!: (response: Response) => void;
  const saveResponse = new Promise<Response>((resolve) => {
    completeSave = resolve;
  });
  const fetchMock = vi
    .fn()
    .mockResolvedValueOnce(
      new Response(JSON.stringify({ entries: [] }), {
        status: 200,
        headers: { etag: '"rev-1"' },
      }),
    )
    .mockReturnValueOnce(saveResponse);
  vi.stubGlobal("fetch", fetchMock);
  render(
    <AccessControl
      apiBase="/api/v3"
      noteId={NOTE_ID}
      revision={1}
      onRevision={vi.fn()}
    />,
  );

  const save = await screen.findByRole("button", {
    name: "共有設定を保存",
  });
  fireEvent.click(save);
  expect(
    await screen.findByRole("button", { name: "保存しています…" }),
  ).toBeDisabled();
  fireEvent.click(save);
  expect(fetchMock).toHaveBeenCalledTimes(2);

  completeSave(
    new Response(
      JSON.stringify({
        note_id: NOTE_ID,
        title: "共有",
        source: "= 共有\n\n本文",
        tags: [],
        created_at_ms: 1,
        updated_at_ms: 2,
        revision: 2,
        created_via: "web",
        review_status: "pending",
        reviewed_revision: null,
        reviewed_at_ms: null,
      }),
      { status: 200 },
    ),
  );
  await screen.findByText("共有設定を保存しました。");
});

test("別のノートへ切り替えた後に古い保存結果を適用しない", async () => {
  const nextNoteId = "0197c9bc-0000-7000-8000-000000000002";
  let completeSave!: (response: Response) => void;
  const saveResponse = new Promise<Response>((resolve) => {
    completeSave = resolve;
  });
  const fetchMock = vi
    .fn()
    .mockResolvedValueOnce(
      new Response(JSON.stringify({ entries: [] }), {
        status: 200,
        headers: { etag: '"rev-1"' },
      }),
    )
    .mockReturnValueOnce(saveResponse)
    .mockResolvedValueOnce(
      new Response(JSON.stringify({ entries: [] }), {
        status: 200,
        headers: { etag: '"rev-7"' },
      }),
    );
  vi.stubGlobal("fetch", fetchMock);
  const onRevision = vi.fn();
  const { rerender } = render(
    <AccessControl
      apiBase="/api/v3"
      noteId={NOTE_ID}
      revision={1}
      onRevision={onRevision}
    />,
  );
  fireEvent.click(
    await screen.findByRole("button", { name: "共有設定を保存" }),
  );
  rerender(
    <AccessControl
      apiBase="/api/v3"
      noteId={nextNoteId}
      revision={7}
      onRevision={onRevision}
    />,
  );
  await waitFor(() => expect(fetchMock).toHaveBeenCalledTimes(3));

  completeSave(
    new Response(
      JSON.stringify({
        note_id: NOTE_ID,
        title: "以前のノート",
        source: "= 以前のノート\n\n本文",
        tags: [],
        created_at_ms: 1,
        updated_at_ms: 2,
        revision: 2,
        created_via: "web",
        review_status: "pending",
        reviewed_revision: null,
        reviewed_at_ms: null,
      }),
      { status: 200 },
    ),
  );
  await Promise.resolve();
  expect(onRevision).not.toHaveBeenCalled();
  expect(screen.queryByText("共有設定を保存しました。")).toBeNull();
});
