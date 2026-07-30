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
  vi.unstubAllGlobals();
});

describe("Application", () => {
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
              tags: [],
              created_at_ms: 1,
              updated_at_ms: 1,
              revision: 1,
            },
            access: "manage",
            html: "<article><h1>設計メモ</h1></article>",
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
    expect(
      screen.getByText("0197c9bc-0000-7000-8000-000000000001"),
    ).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "note IDをコピー" }));

    await waitFor(() =>
      expect(writeText).toHaveBeenCalledWith(
        "0197c9bc-0000-7000-8000-000000000001",
      ),
    );
    expect(screen.getByRole("status", { name: "" })).toHaveTextContent(
      "note IDをコピーしました。",
    );
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

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "note IDをコピーできませんでした。",
    );
  });
});
