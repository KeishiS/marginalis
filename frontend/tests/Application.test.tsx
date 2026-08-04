import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { Application } from "../src/Application";

const config = {
  apiBase: "/marginalis/api/v3",
  basePath: "/marginalis",
  path: "/",
  search: "",
  styleNonce: "test-style-nonce",
};

afterEach(() => {
  cleanup();
  document.cookie = "marginalis_csrf=; Max-Age=0; path=/";
  vi.unstubAllGlobals();
});

describe("Application", () => {
  it("所有者の数式マクロ設定を読み込み、定義例を追加できる", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue(
        new Response(JSON.stringify({ macros: [], revision: 0 }), {
          status: 200,
          headers: { "content-type": "application/json" },
        }),
      ),
    );

    render(
      <Application config={{ ...config, path: "/settings/math-macros" }} />,
    );

    fireEvent.click(await screen.findByRole("button", { name: /argmax/ }));
    expect(screen.getByLabelText("コマンド名")).toHaveValue("argmax");
    expect(screen.getByLabelText("置換内容")).toHaveValue(
      String.raw`\operatorname*{arg\,max}`,
    );
  });

  it("一覧を取得し、サブパスを保ったリンクを表示する", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue(
        new Response(
          JSON.stringify([
            {
              note_id: "0197c9bc-0000-7000-8000-000000000001",
              title: "設計メモ",
              source: "= 設計メモ\n",
              tags: [],
              updated_at_ms: 1,
              revision: 1,
              access: "manage",
            },
          ]),
          { status: 200, headers: { "content-type": "application/json" } },
        ),
      ),
    );

    render(<Application config={config} />);

    const link = await screen.findByRole("link", { name: "設計メモ" });
    expect(link).toHaveAttribute(
      "href",
      "/marginalis/notes/0197c9bc-0000-7000-8000-000000000001",
    );
  });

  it("一覧のタグ、更新日時、アクセス水準と絞り込み状態を表示する", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue(
        new Response(
          JSON.stringify([
            {
              note_id: "0197c9bc-0000-7000-8000-000000000001",
              title: "Rustメモ",
              tags: ["research", "rust"],
              updated_at_ms: Date.parse("2026-07-28T12:00:00Z"),
              revision: 1,
              access: "edit",
            },
            {
              note_id: "0197c9bc-0000-7000-8000-000000000002",
              title: "対象外",
              tags: ["other"],
              updated_at_ms: Date.parse("2026-07-28T12:00:00Z"),
              revision: 1,
              access: "read",
            },
          ]),
          { status: 200, headers: { "content-type": "application/json" } },
        ),
      ),
    );

    render(
      <Application
        config={{ ...config, search: "?tag=research&updated_after=2026-07-01" }}
      />,
    );

    expect(
      await screen.findByRole("link", { name: "Rustメモ" }),
    ).toHaveAttribute(
      "href",
      "/marginalis/notes/0197c9bc-0000-7000-8000-000000000001?tag=research&updated_after=2026-07-01",
    );
    expect(
      screen.queryByRole("link", { name: "対象外" }),
    ).not.toBeInTheDocument();
    expect(screen.getByText("編集")).toBeInTheDocument();
    expect(screen.getByText("rust")).toBeInTheDocument();
    expect(screen.getByLabelText("タグ", { selector: "input" })).toHaveValue(
      "research",
    );
  });

  it("閲覧APIの実効権限に応じて操作リンクを表示する", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    vi.stubGlobal("navigator", {
      ...navigator,
      clipboard: { writeText },
    });
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue(
        new Response(
          JSON.stringify({
            note: {
              note_id: "0197c9bc-0000-7000-8000-000000000001",
              title: "設計メモ",
              source: "= 設計メモ\n\n本文",
              tags: ["研究", "Rust"],
              created_at_ms: 1,
              updated_at_ms: 1,
              revision: 1,
            },
            access: "manage",
            html: "<article><h1>設計メモ</h1></article>",
            math_macros: [],
            related: { outgoing: [], incoming: [] },
          }),
          { status: 200, headers: { "content-type": "application/json" } },
        ),
      ),
    );

    await act(async () => {
      render(
        <Application
          config={{
            ...config,
            path: "/notes/0197c9bc-0000-7000-8000-000000000001",
          }}
        />,
      );
    });

    await waitFor(() =>
      expect(screen.getByRole("link", { name: "編集" })).toBeInTheDocument(),
    );
    expect(screen.getByRole("link", { name: "共有設定" })).toBeInTheDocument();
    const deleteButton = screen.getByRole("button", { name: "削除" });
    expect(
      screen.getByText("0197c9bc-0000-7000-8000-000000000001"),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("list", { name: "ノートのタグ" }).querySelectorAll("li"),
    ).toHaveLength(2);
    expect(
      screen
        .getByRole("list", { name: "ノートのタグ" })
        .querySelectorAll("li")[0],
    ).toHaveTextContent("研究");
    expect(
      screen
        .getByRole("list", { name: "ノートのタグ" })
        .querySelectorAll("li")[1],
    ).toHaveTextContent("Rust");

    fireEvent.click(screen.getByRole("button", { name: "note IDをコピー" }));

    await waitFor(() =>
      expect(writeText).toHaveBeenCalledWith(
        "0197c9bc-0000-7000-8000-000000000001",
      ),
    );
    expect(screen.getByRole("status", { name: "" })).toHaveTextContent(
      "note IDをコピーしました。",
    );

    fireEvent.click(deleteButton);
    const dialog = screen.getByRole("dialog", {
      name: "このノートを削除しますか？",
    });
    expect(dialog).toHaveTextContent("設計メモ");
    expect(dialog).toHaveTextContent("削除後30日以内");
    const cancelButton = screen.getByRole("button", { name: "取り消す" });
    expect(cancelButton).toHaveFocus();
    fireEvent.keyDown(dialog, { key: "Tab", shiftKey: true });
    expect(screen.getByRole("button", { name: "削除する" })).toHaveFocus();
    fireEvent.keyDown(dialog, { key: "Tab" });
    expect(cancelButton).toHaveFocus();
    fireEvent.click(cancelButton);
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(deleteButton).toHaveFocus();
  });

  it("note IDをコピーできない場合に失敗を通知する", async () => {
    vi.stubGlobal("navigator", {
      ...navigator,
      clipboard: {
        writeText: vi.fn().mockRejectedValue(new Error("denied")),
      },
    });
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue(
        new Response(
          JSON.stringify({
            note: {
              note_id: "0197c9bc-0000-7000-8000-000000000001",
              title: "設計メモ",
              source: "= 設計メモ\n\n本文",
              tags: [],
              created_at_ms: 1,
              updated_at_ms: 1,
              revision: 1,
            },
            access: "read",
            html: "<article><h1>設計メモ</h1></article>",
            math_macros: [],
            related: { outgoing: [], incoming: [] },
          }),
          { status: 200, headers: { "content-type": "application/json" } },
        ),
      ),
    );

    render(
      <Application
        config={{
          ...config,
          path: "/notes/0197c9bc-0000-7000-8000-000000000001",
        }}
      />,
    );
    fireEvent.click(
      await screen.findByRole("button", { name: "note IDをコピー" }),
    );
    expect(
      screen.queryByRole("list", { name: "ノートのタグ" }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "削除" }),
    ).not.toBeInTheDocument();

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "note IDをコピーできませんでした。",
    );
  });

  it("共有された編集者には削除操作を表示しない", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue(
        new Response(
          JSON.stringify({
            note: {
              note_id: "0197c9bc-0000-7000-8000-000000000001",
              title: "共有ノート",
              source: "= 共有ノート\n",
              tags: [],
              created_at_ms: 1,
              updated_at_ms: 1,
              revision: 2,
            },
            access: "edit",
            html: "<article></article>",
            math_macros: [],
            related: { outgoing: [], incoming: [] },
          }),
          { status: 200, headers: { "content-type": "application/json" } },
        ),
      ),
    );

    render(
      <Application
        config={{
          ...config,
          path: "/notes/0197c9bc-0000-7000-8000-000000000001",
        }}
      />,
    );

    expect(
      await screen.findByRole("link", { name: "編集" }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "削除" }),
    ).not.toBeInTheDocument();
  });

  it("revision競合では内容を残して再読み込みを案内する", async () => {
    document.cookie = "marginalis_csrf=test-csrf; path=/";
    const fetch = vi
      .fn()
      .mockResolvedValueOnce(
        new Response(
          JSON.stringify({
            note: {
              note_id: "0197c9bc-0000-7000-8000-000000000001",
              title: "競合するノート",
              source: "= 競合するノート\n",
              tags: [],
              created_at_ms: 1,
              updated_at_ms: 1,
              revision: 7,
            },
            access: "manage",
            html: "<article><p>残す本文</p></article>",
            math_macros: [],
            related: { outgoing: [], incoming: [] },
          }),
          { status: 200, headers: { "content-type": "application/json" } },
        ),
      )
      .mockResolvedValueOnce(
        new Response(
          JSON.stringify({
            code: "conflict",
            message: "note revision conflicts",
          }),
          { status: 409, headers: { "content-type": "application/json" } },
        ),
      );
    vi.stubGlobal("fetch", fetch);

    render(
      <Application
        config={{
          ...config,
          path: "/notes/0197c9bc-0000-7000-8000-000000000001",
        }}
      />,
    );
    fireEvent.click(await screen.findByRole("button", { name: "削除" }));
    fireEvent.click(screen.getByRole("button", { name: "削除する" }));

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "画面を再読み込みしてから",
    );
    expect(screen.getByText("残す本文")).toBeInTheDocument();
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    expect(fetch).toHaveBeenLastCalledWith(
      "/marginalis/api/v3/notes/0197c9bc-0000-7000-8000-000000000001",
      expect.objectContaining({
        method: "DELETE",
        headers: expect.objectContaining({
          "if-match": '"rev-7"',
          "x-csrf-token": "test-csrf",
        }),
      }),
    );
    expect(screen.getByRole("button", { name: "削除する" })).toBeEnabled();
  });
});
