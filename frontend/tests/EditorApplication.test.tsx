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
import { Note, NoteDiagnostic } from "../src/api";

vi.mock("../src/AsciiDocEditor", async () => {
  const React = await import("react");
  return {
    AsciiDocEditor: React.forwardRef(function MockAsciiDocEditor(
      {
        value,
        diagnostics,
        disabled,
        labelledBy,
        onChange,
        onCompositionChange,
        onSave,
      }: {
        value: string;
        diagnostics: NoteDiagnostic[];
        disabled: boolean;
        labelledBy: string;
        onChange: (value: string) => void;
        onCompositionChange: (composing: boolean) => void;
        onSave: () => void;
      },
      forwardedRef: React.ForwardedRef<{
        focus: () => void;
        selectRange: (anchor: number, head: number) => void;
        setScrollRatio: () => void;
      }>,
    ) {
      const input = React.useRef<HTMLTextAreaElement>(null);
      React.useImperativeHandle(
        forwardedRef,
        () => ({
          focus() {
            input.current?.focus();
          },
          selectRange(anchor, head) {
            input.current?.focus();
            input.current?.setSelectionRange(anchor, head);
          },
          setScrollRatio() {},
        }),
        [],
      );
      return (
        <textarea
          ref={input}
          aria-labelledby={labelledBy}
          data-inline-diagnostics={
            diagnostics.filter(
              (diagnostic) =>
                diagnostic.target.field === "source" &&
                diagnostic.span?.unit === "utf8_byte",
            ).length
          }
          value={value}
          disabled={disabled}
          onChange={(event) => onChange(event.target.value)}
          onCompositionStart={() => onCompositionChange(true)}
          onCompositionEnd={() => onCompositionChange(false)}
          onKeyDown={(event) => {
            if (
              (event.ctrlKey || event.metaKey) &&
              event.key.toLowerCase() === "s"
            ) {
              event.preventDefault();
              onSave();
            }
          }}
        />
      );
    }),
  };
});

const CONFIG: EditorConfig = {
  mode: "create",
  noteId: "",
  apiBase: "/marginalis/api/v3",
  basePath: "/marginalis",
  search: "",
  styleNonce: "test-style-nonce",
};

const SOURCE =
  "= 既存の題名\n:tags: 研究, 試験\n:sectnums:\n\n== 見出し\n\n既存の本文";
const NOTE: Note = {
  note_id: "0197c9bc-0000-7000-8000-000000000001",
  title: "既存の題名",
  source: SOURCE,
  tags: ["研究", "試験"],
  created_at_ms: 1,
  updated_at_ms: 2,
  revision: 3,
};

afterEach(() => {
  cleanup();
  vi.useRealTimers();
  vi.unstubAllGlobals();
  window.history.replaceState(null, "", "/");
});

test("完全なAsciiDoc文書を一つの入力として作成する", async () => {
  document.cookie = "marginalis_csrf=test-csrf";
  const fetchMock = vi
    .fn<typeof fetch>()
    .mockResolvedValue(jsonResponse(NOTE, 201));
  vi.stubGlobal("fetch", fetchMock);
  render(<EditorApplication config={CONFIG} />);

  const editor = screen.getByRole("textbox", { name: "AsciiDoc文書" });
  fireEvent.change(editor, { target: { value: SOURCE } });
  fireEvent.click(screen.getByRole("button", { name: "保存" }));

  expect(await screen.findByText("保存しました。")).toBeInTheDocument();
  expect(fetchMock).toHaveBeenCalledWith(
    "/marginalis/api/v3/notes",
    expect.objectContaining({
      method: "POST",
      body: JSON.stringify({ source: SOURCE }),
    }),
  );
});

test("キーボード保存の成功を右上の通知で伝える", async () => {
  const fetchMock = vi
    .fn<typeof fetch>()
    .mockResolvedValueOnce(jsonResponse(NOTE, 201))
    .mockResolvedValueOnce(
      jsonResponse({ code: "internal_error", message: "failed" }, 500),
    );
  vi.stubGlobal("fetch", fetchMock);
  render(<EditorApplication config={CONFIG} />);

  const editor = screen.getByRole("textbox", { name: "AsciiDoc文書" });
  fireEvent.change(editor, { target: { value: SOURCE } });
  fireEvent.keyDown(editor, { key: "s", ctrlKey: true });

  const message = await screen.findByText("保存しました。");
  expect(message.closest(".toast")).toBeInTheDocument();
  expect(message.closest(".toast-region")).toHaveAttribute(
    "aria-live",
    "polite",
  );
  expect(fetchMock).toHaveBeenCalledTimes(1);
  expect(screen.getByText("変更は保存されています。")).toHaveAttribute(
    "role",
    "status",
  );

  fireEvent.change(editor, { target: { value: `${SOURCE}\n\n追記` } });
  fireEvent.click(screen.getByRole("button", { name: "保存" }));
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "保存できませんでした",
  );
  expect(screen.queryByText("保存しました。")).not.toBeInTheDocument();
});

test("既存文書を読み込み、更新番号を付けて保存する", async () => {
  const updatedSource = SOURCE.replace("既存の本文", "更新後");
  const fetchMock = vi
    .fn<typeof fetch>()
    .mockResolvedValueOnce(jsonResponse(NOTE))
    .mockResolvedValueOnce(
      jsonResponse({ ...NOTE, source: updatedSource, revision: 4 }),
    );
  vi.stubGlobal("fetch", fetchMock);
  render(
    <EditorApplication
      config={{ ...CONFIG, mode: "edit", noteId: NOTE.note_id }}
    />,
  );

  await screen.findByText("更新番号: 3");
  const editor = screen.getByRole("textbox", { name: "AsciiDoc文書" });
  expect(editor).toHaveValue(SOURCE);
  fireEvent.change(editor, { target: { value: updatedSource } });
  fireEvent.click(screen.getByRole("button", { name: "保存" }));

  await waitFor(() => expect(fetchMock).toHaveBeenCalledTimes(2));
  expect(fetchMock.mock.calls[1]?.[1]).toEqual(
    expect.objectContaining({
      method: "PUT",
      body: JSON.stringify({ source: updatedSource }),
      headers: expect.objectContaining({ "if-match": '"rev-3"' }),
    }),
  );
});

test("競合時も編集中の完全な文書を維持する", async () => {
  const local = SOURCE.replace("既存の本文", "編集中");
  const current = {
    ...NOTE,
    source: SOURCE.replace("既存の本文", "現在"),
    revision: 4,
  };
  const fetchMock = vi
    .fn<typeof fetch>()
    .mockResolvedValueOnce(jsonResponse(NOTE))
    .mockResolvedValueOnce(
      jsonResponse({ code: "conflict", message: "conflict" }, 409),
    )
    .mockResolvedValueOnce(jsonResponse(current));
  vi.stubGlobal("fetch", fetchMock);
  render(
    <EditorApplication
      config={{ ...CONFIG, mode: "edit", noteId: NOTE.note_id }}
    />,
  );

  await screen.findByText("更新番号: 3");
  const editor = screen.getByRole("textbox", { name: "AsciiDoc文書" });
  expect(editor).toHaveValue(SOURCE);
  fireEvent.change(editor, { target: { value: local } });
  fireEvent.click(screen.getByRole("button", { name: "保存" }));

  expect(
    await screen.findByRole("heading", { name: "更新内容の競合" }),
  ).toBeInTheDocument();
  expect(editor).toHaveValue(local);
  expect(screen.getByRole("table")).toHaveTextContent("編集中");
  expect(screen.getByRole("table")).toHaveTextContent("現在");
});

test("診断からUTF-8位置に対応する入力範囲へ移動する", async () => {
  const source = "= 題名\n\n日本語";
  const start = new TextEncoder().encode("= 題名\n\n").length;
  vi.stubGlobal(
    "fetch",
    vi.fn<typeof fetch>().mockResolvedValue(
      jsonResponse(
        {
          code: "validation_failed",
          message: "invalid",
          diagnostics: [
            {
              code: "asciidoc_parse_failed",
              severity: "error",
              target: { field: "source" },
              span: { start, end: start + 6, unit: "utf8_byte" },
              message: "invalid source",
            },
          ],
        },
        422,
      ),
    ),
  );
  render(<EditorApplication config={CONFIG} />);
  const editor = screen.getByRole<HTMLTextAreaElement>("textbox", {
    name: "AsciiDoc文書",
  });
  fireEvent.change(editor, { target: { value: source } });
  fireEvent.click(screen.getByRole("button", { name: "プレビュー" }));
  fireEvent.click(screen.getByRole("button", { name: "保存" }));

  expect(await screen.findByRole("alert")).toHaveTextContent(
    "エラー: 3行1列: AsciiDoc本文を解析できませんでした。",
  );
  fireEvent.click(screen.getByRole("button", { name: "入力位置へ移動" }));
  expect(document.querySelector(".editor-workspace")).toHaveAttribute(
    "data-view-mode",
    "write",
  );
  await waitFor(() => {
    expect(editor).toHaveFocus();
    expect(editor.value.slice(editor.selectionStart, editor.selectionEnd)).toBe(
      "日本",
    );
  });
});

test("プレビュー警告を入力欄へ渡して修正後にすぐ取り除く", async () => {
  vi.useFakeTimers();
  const source =
    "= 調査結果\n\nこの結果はxref:note:0197c9bc-0000-7000-8000-000000000002[先行調査]";
  const start = new TextEncoder().encode("= 調査結果\n\nこの結果は").length;
  const fetchMock = vi
    .fn<typeof fetch>()
    .mockResolvedValueOnce(
      jsonResponse({
        html: "<p>この結果はxref:...</p>",
        diagnostics: [
          {
            code: "macro-boundary",
            severity: "warning",
            target: { field: "source" },
            span: { start, end: start + 4, unit: "utf8_byte" },
            message: "a space is required before the inline macro",
          },
        ],
      }),
    )
    .mockResolvedValueOnce(
      jsonResponse({ html: "<p>この結果は xref:...</p>", diagnostics: [] }),
    );
  vi.stubGlobal("fetch", fetchMock);
  render(<EditorApplication config={CONFIG} />);

  const editor = screen.getByRole<HTMLTextAreaElement>("textbox", {
    name: "AsciiDoc文書",
  });
  fireEvent.change(editor, { target: { value: source } });
  await act(async () => {
    await vi.advanceTimersByTimeAsync(350);
  });

  expect(editor).toHaveAttribute("data-inline-diagnostics", "1");
  expect(
    screen.queryByRole("heading", { name: "入力時の診断" }),
  ).not.toBeInTheDocument();

  fireEvent.change(editor, {
    target: { value: source.replace("はxref:", "は xref:") },
  });
  expect(
    screen.queryByRole("heading", { name: "入力時の診断" }),
  ).not.toBeInTheDocument();
  expect(editor).toHaveAttribute("data-inline-diagnostics", "0");
  await act(async () => {
    await vi.advanceTimersByTimeAsync(350);
  });
  expect(
    screen.queryByRole("heading", { name: "入力時の診断" }),
  ).not.toBeInTheDocument();
});

test("プレビュー失敗時も最後に成功した表示を残す", async () => {
  vi.useFakeTimers();
  const fetchMock = vi
    .fn<typeof fetch>()
    .mockResolvedValueOnce(
      jsonResponse({ html: "<p>成功した表示</p>", diagnostics: [] }),
    )
    .mockResolvedValueOnce(
      jsonResponse(
        {
          code: "validation_failed",
          message: "invalid",
          diagnostics: [],
        },
        422,
      ),
    );
  vi.stubGlobal("fetch", fetchMock);
  render(<EditorApplication config={CONFIG} />);

  await act(async () => {
    await vi.advanceTimersByTimeAsync(350);
  });
  expect(screen.getByText("成功した表示")).toBeInTheDocument();

  fireEvent.change(screen.getByRole("textbox", { name: "AsciiDoc文書" }), {
    target: { value: "= 不正\n\ninclude::secret[]" },
  });
  await act(async () => {
    await vi.advanceTimersByTimeAsync(350);
  });
  expect(
    screen.getByText("最後に成功したプレビューを表示しています。"),
  ).toBeInTheDocument();
  expect(screen.getByText("成功した表示")).toBeInTheDocument();
  expect(
    screen.getByRole("heading", { name: "プレビューできませんでした" }),
  ).toBeInTheDocument();
});

test("表示を切り替えても入力欄を維持する", async () => {
  vi.stubGlobal("fetch", vi.fn<typeof fetch>());
  render(<EditorApplication config={CONFIG} />);

  const workspace = document.querySelector(".editor-workspace");
  const editor = screen.getByRole("textbox", { name: "AsciiDoc文書" });
  expect(workspace).toHaveAttribute("data-view-mode", "split");
  expect(screen.getByRole("button", { name: "分割" })).toHaveAttribute(
    "aria-pressed",
    "true",
  );

  fireEvent.click(screen.getByRole("button", { name: "プレビュー" }));
  expect(workspace).toHaveAttribute("data-view-mode", "preview");
  expect(editor).toBeInTheDocument();

  fireEvent.click(screen.getByRole("button", { name: "執筆" }));
  expect(workspace).toHaveAttribute("data-view-mode", "write");
  await waitFor(() => expect(editor).toHaveFocus());
});

test("狭い画面では分割せず執筆とプレビューを明示的に切り替える", () => {
  vi.stubGlobal(
    "matchMedia",
    vi.fn(() => ({
      matches: true,
      media: "(max-width: 60rem)",
      onchange: null,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
      addListener: vi.fn(),
      removeListener: vi.fn(),
      dispatchEvent: vi.fn(),
    })),
  );
  vi.stubGlobal("fetch", vi.fn<typeof fetch>());
  render(<EditorApplication config={CONFIG} />);

  expect(document.querySelector(".editor-workspace")).toHaveAttribute(
    "data-view-mode",
    "write",
  );
  expect(screen.getByRole("button", { name: "分割" })).toBeDisabled();
  expect(
    screen.getByText("この画面幅では執筆表示に切り替えています。"),
  ).toBeInTheDocument();

  fireEvent.click(screen.getByRole("button", { name: "プレビュー" }));
  expect(document.querySelector(".editor-workspace")).toHaveAttribute(
    "data-view-mode",
    "preview",
  );
});

function jsonResponse(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "content-type": "application/json" },
  });
}
