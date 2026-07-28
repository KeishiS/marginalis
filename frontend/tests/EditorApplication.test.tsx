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

test("revision競合時に三つの内容を比較し、明示操作後に再保存する", async () => {
  document.cookie = "marginalis_csrf=test-csrf";
  const localBody = `編集中😀\n${"長い行".repeat(80)}`;
  const current = {
    ...NOTE,
    title: "現在の題名",
    body: "現在の本文\n追加行",
    tags: ["共有", "更新済み"],
    revision: 4,
  };
  const saved = {
    ...current,
    title: "編集中の題名",
    body: localBody,
    tags: ["研究", "試験"],
    revision: 5,
  };
  const fetchMock = vi
    .fn<typeof fetch>()
    .mockResolvedValueOnce(jsonResponse(NOTE))
    .mockResolvedValueOnce(
      jsonResponse(
        { code: "conflict", message: "note revision conflicts" },
        409,
      ),
    )
    .mockResolvedValueOnce(jsonResponse(current))
    .mockResolvedValueOnce(jsonResponse(saved));
  vi.stubGlobal("fetch", fetchMock);
  render(
    <EditorApplication
      config={{ ...CREATE_CONFIG, mode: "edit", noteId: NOTE.note_id }}
    />,
  );

  expect(await screen.findByDisplayValue("既存の題名")).toBeInTheDocument();
  fireEvent.change(screen.getByRole("textbox", { name: "題名" }), {
    target: { value: "編集中の題名" },
  });
  fireEvent.change(screen.getByRole("textbox", { name: "本文（AsciiDoc）" }), {
    target: { value: localBody },
  });
  fireEvent.click(screen.getByRole("button", { name: "保存" }));

  const conflictHeading = await screen.findByRole("heading", {
    name: "更新内容の競合",
  });
  expect(conflictHeading).toHaveFocus();
  expect(
    screen.getByRole("region", { name: "編集開始時点" }),
  ).toHaveTextContent("既存の題名");
  expect(screen.getByDisplayValue("編集中の題名")).toBeInTheDocument();
  expect(screen.getByRole("region", { name: "編集中" })).toHaveTextContent(
    "編集中",
  );
  const currentRegion = screen.getByRole("region", {
    name: "現在保存されている内容",
  });
  expect(currentRegion).toHaveTextContent("現在の題名");
  expect(currentRegion).toHaveTextContent("共有, 更新済み");
  expect(currentRegion).toHaveTextContent("変更あり");
  expect(screen.getByRole("table")).toHaveTextContent("編集中😀");
  expect(screen.getByRole("table")).toHaveTextContent("追加行");
  expect(screen.getByLabelText("本文比較表のスクロール領域")).toHaveAttribute(
    "tabindex",
    "0",
  );
  expect(screen.getByRole("textbox", { name: "本文（AsciiDoc）" })).toHaveValue(
    localBody,
  );

  fireEvent.click(
    screen.getByRole("button", { name: "更新番号4を編集の基準にする" }),
  );
  expect(fetchMock).toHaveBeenCalledTimes(3);
  expect(
    screen.getByText(
      "更新番号4を基準にしました。内容を確認して保存してください。",
    ),
  ).toBeInTheDocument();
  expect(screen.getByRole("textbox", { name: "本文（AsciiDoc）" })).toHaveValue(
    localBody,
  );

  fireEvent.click(screen.getByRole("button", { name: "保存" }));
  await waitFor(() => expect(fetchMock).toHaveBeenCalledTimes(4));
  expect(fetchMock.mock.calls[3]?.[1]).toEqual(
    expect.objectContaining({
      method: "PUT",
      body: JSON.stringify({
        title: "編集中の題名",
        body: localBody,
        tags: ["研究", "試験"],
        expected_revision: 4,
      }),
    }),
  );
  expect(await screen.findByText("更新番号: 5")).toBeInTheDocument();
});

test("競合確認後に再更新された場合も最新revisionを再取得する", async () => {
  const current4 = { ...NOTE, title: "現在4", revision: 4 };
  const current5 = { ...NOTE, title: "現在5", revision: 5 };
  const fetchMock = vi
    .fn<typeof fetch>()
    .mockResolvedValueOnce(jsonResponse(NOTE))
    .mockResolvedValueOnce(
      jsonResponse(
        { code: "conflict", message: "note revision conflicts" },
        409,
      ),
    )
    .mockResolvedValueOnce(jsonResponse(current4))
    .mockResolvedValueOnce(
      jsonResponse(
        { code: "conflict", message: "note revision conflicts" },
        409,
      ),
    )
    .mockResolvedValueOnce(jsonResponse(current5));
  vi.stubGlobal("fetch", fetchMock);
  render(
    <EditorApplication
      config={{ ...CREATE_CONFIG, mode: "edit", noteId: NOTE.note_id }}
    />,
  );

  expect(await screen.findByDisplayValue("既存の題名")).toBeInTheDocument();
  fireEvent.change(screen.getByRole("textbox", { name: "題名" }), {
    target: { value: "編集中" },
  });
  fireEvent.click(screen.getByRole("button", { name: "保存" }));
  fireEvent.click(
    await screen.findByRole("button", {
      name: "更新番号4を編集の基準にする",
    }),
  );
  fireEvent.click(screen.getByRole("button", { name: "保存" }));

  expect(
    await screen.findByRole("button", {
      name: "更新番号5を編集の基準にする",
    }),
  ).toBeInTheDocument();
  expect(fetchMock.mock.calls[3]?.[1]).toEqual(
    expect.objectContaining({
      body: expect.stringContaining('"expected_revision":4'),
    }),
  );
  expect(screen.getByDisplayValue("編集中")).toBeInTheDocument();
});

test("挿入と削除をLCSで整列し、変更状態を文字で表示する", async () => {
  const baseline = { ...NOTE, body: "A\r\nB" };
  const current = { ...NOTE, body: "A\nB\nY", revision: 4 };
  const fetchMock = vi
    .fn<typeof fetch>()
    .mockResolvedValueOnce(jsonResponse(baseline))
    .mockResolvedValueOnce(
      jsonResponse(
        { code: "conflict", message: "note revision conflicts" },
        409,
      ),
    )
    .mockResolvedValueOnce(jsonResponse(current));
  vi.stubGlobal("fetch", fetchMock);
  render(
    <EditorApplication
      config={{ ...CREATE_CONFIG, mode: "edit", noteId: NOTE.note_id }}
    />,
  );

  expect(await screen.findByDisplayValue("既存の題名")).toBeInTheDocument();
  expect(screen.getByRole("textbox", { name: "本文（AsciiDoc）" })).toHaveValue(
    "A\nB",
  );
  fireEvent.change(screen.getByRole("textbox", { name: "本文（AsciiDoc）" }), {
    target: { value: "X\nA\nB" },
  });
  fireEvent.click(screen.getByRole("button", { name: "保存" }));

  const table = await screen.findByRole("table", {
    name: "本文の行単位比較",
  });
  expect(table).toHaveTextContent("編集中に追加");
  expect(table).toHaveTextContent("現在の内容に追加");
  expect(table.querySelectorAll("tr.changed")).toHaveLength(2);
  expect(table).not.toHaveTextContent("編集中から削除");
  expect(table).not.toHaveTextContent("現在の内容から削除");
});

test("競合後に最新内容を閲覧できない場合は比較情報を表示しない", async () => {
  const fetchMock = vi
    .fn<typeof fetch>()
    .mockResolvedValueOnce(jsonResponse(NOTE))
    .mockResolvedValueOnce(
      jsonResponse(
        { code: "conflict", message: "note revision conflicts" },
        409,
      ),
    )
    .mockResolvedValueOnce(
      jsonResponse(
        { code: "not_found", message: "note is not available" },
        404,
      ),
    );
  vi.stubGlobal("fetch", fetchMock);
  render(
    <EditorApplication
      config={{ ...CREATE_CONFIG, mode: "edit", noteId: NOTE.note_id }}
    />,
  );

  expect(await screen.findByDisplayValue("既存の題名")).toBeInTheDocument();
  fireEvent.change(screen.getByRole("textbox", { name: "題名" }), {
    target: { value: "保持する編集中の題名" },
  });
  fireEvent.click(screen.getByRole("button", { name: "保存" }));

  expect(await screen.findByText("note is not available")).toBeInTheDocument();
  expect(screen.getByDisplayValue("保持する編集中の題名")).toBeInTheDocument();
  expect(
    screen.queryByRole("heading", { name: "更新内容の競合" }),
  ).not.toBeInTheDocument();
  expect(
    screen.queryByRole("button", { name: /編集の基準にする/ }),
  ).not.toBeInTheDocument();
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
