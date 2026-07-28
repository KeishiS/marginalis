import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import { EditorApplication, EditorConfig } from "../src/EditorApplication";
import { Note } from "../src/api";

const CONFIG: EditorConfig = {
  mode: "create",
  noteId: "",
  apiBase: "/marginalis/api/v3",
  basePath: "/marginalis",
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

function jsonResponse(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "content-type": "application/json" },
  });
}
