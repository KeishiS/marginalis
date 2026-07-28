import { act, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { Application } from "../src/Application";

const config = {
  apiBase: "/marginalis/api/v3",
  basePath: "/marginalis",
  path: "/",
};

afterEach(() => {
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

  it("閲覧APIの実効権限に応じて操作リンクを表示する", async () => {
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
  });
});
