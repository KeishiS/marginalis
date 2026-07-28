import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import { EditorApplication, EditorConfig } from "../src/EditorApplication";
import { Note } from "../src/api";
import { utf8ByteOffsetToLineColumn } from "../src/textPosition";

const CREATE_CONFIG: EditorConfig = {
  mode: "create",
  noteId: "",
  apiBase: "/marginalis/api/v2",
  basePath: "/marginalis",
};

const NOTE: Note = {
  note_id: "0197c9bc-0000-7000-8000-000000000001",
  title: "既存の題名",
  body: "既存の本文",
  tags: ["研究", "試験"],
  created_at_ms: 1,
  updated_at_ms: 2,
  revision: 3,
};

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
  vi.useRealTimers();
  window.history.replaceState(null, "", "/");
});

test("新規ノートを明示的に保存してIDと更新番号を反映する", async () => {
  document.cookie = "marginalis_csrf=test-csrf";
  const fetchMock = vi
    .fn<typeof fetch>()
    .mockResolvedValue(jsonResponse(NOTE, 201));
  vi.stubGlobal("fetch", fetchMock);
  render(<EditorApplication config={CREATE_CONFIG} />);

  expect(
    screen.getByRole("heading", { name: "ノートの作成" }),
  ).toBeInTheDocument();
  const save = screen.getByRole("button", { name: "保存" });
  expect(save).toBeDisabled();

  fireEvent.change(screen.getByRole("textbox", { name: "題名" }), {
    target: { value: "新しい題名" },
  });
  fireEvent.change(screen.getByRole("textbox", { name: "本文（AsciiDoc）" }), {
    target: { value: "新しい本文" },
  });
  fireEvent.change(
    screen.getByRole("textbox", { name: "タグ（コンマ区切り）" }),
    { target: { value: "研究, 試験" } },
  );
  expect(screen.getByText("未保存の変更があります。")).toBeInTheDocument();
  fireEvent.click(save);

  expect(await screen.findByText("保存しました。")).toBeInTheDocument();
  expect(screen.getByText("更新番号: 3")).toBeInTheDocument();
  expect(fetchMock).toHaveBeenCalledWith(
    "/marginalis/api/v2/notes",
    expect.objectContaining({
      method: "POST",
      credentials: "same-origin",
      headers: expect.objectContaining({ "x-csrf-token": "test-csrf" }),
      body: JSON.stringify({
        title: "新しい題名",
        body: "新しい本文",
        tags: ["研究", "試験"],
      }),
    }),
  );
  expect(window.location.pathname).toBe(
    "/marginalis/notes/0197c9bc-0000-7000-8000-000000000001/edit",
  );
});

test("既存ノートを取得し、読み込んだrevisionで更新する", async () => {
  document.cookie = "marginalis_csrf=test-csrf";
  const updated = { ...NOTE, title: "更新後", revision: 4 };
  const fetchMock = vi
    .fn<typeof fetch>()
    .mockResolvedValueOnce(jsonResponse(NOTE))
    .mockResolvedValueOnce(jsonResponse(updated));
  vi.stubGlobal("fetch", fetchMock);
  render(
    <EditorApplication
      config={{ ...CREATE_CONFIG, mode: "edit", noteId: NOTE.note_id }}
    />,
  );

  expect(await screen.findByDisplayValue("既存の題名")).toBeInTheDocument();
  fireEvent.change(screen.getByRole("textbox", { name: "題名" }), {
    target: { value: "更新後" },
  });
  fireEvent.click(screen.getByRole("button", { name: "保存" }));

  await waitFor(() => expect(fetchMock).toHaveBeenCalledTimes(2));
  expect(fetchMock.mock.calls[1]).toEqual([
    `/marginalis/api/v2/notes/${NOTE.note_id}`,
    expect.objectContaining({
      method: "PUT",
      body: JSON.stringify({
        title: "更新後",
        body: "既存の本文",
        tags: ["研究", "試験"],
        expected_revision: 3,
      }),
    }),
  ]);
  expect(await screen.findByText("更新番号: 4")).toBeInTheDocument();
});

test("入力検証に失敗しても編集内容と診断を保持する", async () => {
  const fetchMock = vi.fn<typeof fetch>().mockResolvedValue(
    jsonResponse(
      {
        code: "validation_failed",
        message: "note input is invalid",
        diagnostics: [
          {
            code: "invalid_title",
            target: { field: "title" },
            message: "題名を入力してください。",
          },
        ],
      },
      422,
    ),
  );
  vi.stubGlobal("fetch", fetchMock);
  render(<EditorApplication config={CREATE_CONFIG} />);

  fireEvent.change(screen.getByRole("textbox", { name: "題名" }), {
    target: { value: "保持する入力" },
  });
  fireEvent.click(screen.getByRole("button", { name: "保存" }));

  expect(
    await screen.findByRole("heading", { name: "保存できませんでした" }),
  ).toBeInTheDocument();
  expect(
    screen.getByText(
      "題名を入力し、改行と上限を超える文字を取り除いてください。",
    ),
  ).toBeInTheDocument();
  expect(screen.getByDisplayValue("保持する入力")).toBeInTheDocument();
  expect(screen.getByText("未保存の変更があります。")).toBeInTheDocument();
});

test("既存ノートの読込失敗時に新規作成として保存できない", async () => {
  vi.stubGlobal(
    "fetch",
    vi
      .fn<typeof fetch>()
      .mockResolvedValue(
        jsonResponse(
          { code: "not_found", message: "note is not available" },
          404,
        ),
      ),
  );
  render(
    <EditorApplication
      config={{ ...CREATE_CONFIG, mode: "edit", noteId: NOTE.note_id }}
    />,
  );

  expect(
    await screen.findByRole("heading", {
      name: "ノートを読み込めませんでした",
    }),
  ).toBeInTheDocument();
  expect(
    screen.queryByRole("button", { name: "保存" }),
  ).not.toBeInTheDocument();
  expect(screen.getByRole("link", { name: "一覧へ戻る" })).toHaveAttribute(
    "href",
    "/marginalis/",
  );
});

test("入力停止後に最新のプレビューだけを表示する", async () => {
  vi.useFakeTimers();
  const first = deferred<Response>();
  const second = deferred<Response>();
  const fetchMock = vi
    .fn<typeof fetch>()
    .mockReturnValueOnce(first.promise)
    .mockReturnValueOnce(second.promise);
  vi.stubGlobal("fetch", fetchMock);
  const { container } = render(<EditorApplication config={CREATE_CONFIG} />);

  fireEvent.change(screen.getByRole("textbox", { name: "本文（AsciiDoc）" }), {
    target: { value: "最初" },
  });
  await act(() => vi.advanceTimersByTimeAsync(350));
  expect(fetchMock).toHaveBeenCalledTimes(1);

  fireEvent.change(screen.getByRole("textbox", { name: "本文（AsciiDoc）" }), {
    target: { value: "最新" },
  });
  await act(() => vi.advanceTimersByTimeAsync(350));
  expect(fetchMock).toHaveBeenCalledTimes(2);

  await act(async () => {
    second.resolve(jsonResponse({ html: "<p>最新の表示</p>" }));
  });
  expect(container.querySelector(".preview-content")).toHaveTextContent(
    "最新の表示",
  );

  await act(async () => {
    first.resolve(jsonResponse({ html: "<p>古い表示</p>" }));
  });
  expect(container.querySelector(".preview-content")).toHaveTextContent(
    "最新の表示",
  );
});

test("UTF-8バイト位置を日本語、絵文字、CRLFの行と列へ変換する", () => {
  const body = "日本語😀\r\n次の行";
  const offset = new TextEncoder().encode("日本語😀\r\n次").length;

  expect(utf8ByteOffsetToLineColumn(body, offset)).toEqual({
    line: 2,
    column: 2,
  });
});

function jsonResponse(payload: unknown, status = 200): Response {
  return new Response(JSON.stringify(payload), {
    status,
    headers: { "content-type": "application/json" },
  });
}

function deferred<T>(): {
  promise: Promise<T>;
  resolve: (value: T) => void;
} {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((fulfill) => {
    resolve = fulfill;
  });
  return { promise, resolve };
}
