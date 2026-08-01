import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";

import { GraphCanvas } from "../src/graph/GraphCanvas";
import { graphModel } from "../src/graph/model";
import { NoteGraph } from "../src/api";

const NOTE = "0197c9bc-0000-7000-8000-000000000001";
const UPDATED_AT = Date.UTC(2026, 7, 1, 3, 34);

const CONFIG = {
  apiBase: "/api/v3",
  basePath: "/",
  path: "/graph",
  search: "",
  styleNonce: "test-nonce",
};

afterEach(cleanup);

function graph(): NoteGraph {
  return {
    notes: [
      {
        note_id: NOTE,
        title: "先行研究の整理",
        tags: ["研究", "AsciiDoc"],
        updated_at_ms: UPDATED_AT,
      },
    ],
    works: [{ citation_key: "smith2024", title: "An Example Article" }],
    references: [],
    citations: [{ source_note_id: NOTE, citation_key: "smith2024" }],
  };
}

function canvas() {
  return render(<GraphCanvas config={CONFIG} model={graphModel(graph())} />);
}

test("点に触れると、更新日時とタグを吹き出しで示す", () => {
  const { container } = canvas();
  const note = container.querySelector('.graph-vertex[data-kind="note"]');

  expect(container.querySelector(".graph-detail")).toBeNull();
  fireEvent.mouseEnter(note!);

  const detail = container.querySelector(".graph-detail");
  expect(detail?.textContent).toContain("先行研究の整理");
  expect(detail?.textContent).toContain("ノート");
  expect(detail?.textContent).toContain("研究 / AsciiDoc");
  expect(detail?.querySelector("time")?.getAttribute("dateTime")).toBe(
    new Date(UPDATED_AT).toISOString(),
  );

  fireEvent.mouseLeave(note!);
  expect(container.querySelector(".graph-detail")).toBeNull();
});

test("文献の点はcitation keyと題名を示す", () => {
  const { container } = canvas();
  const work = container.querySelector('.graph-vertex[data-kind="work"]');

  fireEvent.mouseEnter(work!);

  const detail = container.querySelector(".graph-detail");
  expect(detail?.textContent).toContain("An Example Article");
  expect(detail?.textContent).toContain("smith2024");
  // 文献には更新日時が無いため、その行を出さない。
  expect(detail?.querySelector("time")).toBeNull();
  expect(detail?.textContent).toContain("なし");
});

test("キーボードのフォーカスでも同じ吹き出しが出る", () => {
  const { container } = canvas();
  const note = container.querySelector('.graph-vertex[data-kind="note"]');

  fireEvent.focus(note!);
  expect(container.querySelector(".graph-detail")).not.toBeNull();

  fireEvent.blur(note!);
  expect(container.querySelector(".graph-detail")).toBeNull();
});

/// 吹き出しはマウスの位置に依存し、読み上げの順序にも乗らない。同じ内容を点の名前にも持たせる。
test("支援技術へは点の名前として同じ内容を伝える", () => {
  canvas();

  const note = screen.getByRole("link", { name: /先行研究の整理/ });
  const label = note.getAttribute("aria-label") ?? "";
  expect(label).toContain("ノート");
  expect(label).toContain("タグ研究、AsciiDoc");
  expect(label).toContain("つながり1件");

  const work = screen.getByRole("link", { name: /An Example Article/ });
  expect(work.getAttribute("aria-label")).toContain("citation key smith2024");
});
