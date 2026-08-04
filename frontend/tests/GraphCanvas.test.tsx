import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import { GraphCanvas } from "../src/graph/GraphCanvas";
import { graphModel } from "../src/graph/model";
import { NoteGraph } from "../src/api";

const NOTE = "0197c9bc-0000-7000-8000-000000000001";
const UPDATED_AT = Date.UTC(2026, 7, 1, 3, 34);
const WORK_TITLE =
  "An Example Article with a Complete Title That Remains Available on Hover";

const CONFIG = {
  apiBase: "/api/v3",
  basePath: "/",
  path: "/graph",
  search: "",
  styleNonce: "test-nonce",
};

afterEach(() => {
  cleanup();
  vi.useRealTimers();
  vi.restoreAllMocks();
});

function rectangle({
  left,
  top,
  width,
  height,
}: {
  left: number;
  top: number;
  width: number;
  height: number;
}): DOMRect {
  return {
    x: left,
    y: top,
    left,
    top,
    width,
    height,
    right: left + width,
    bottom: top + height,
    toJSON: () => ({}),
  };
}

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
    works: [{ citation_key: "smith2024", title: WORK_TITLE }],
    references: [],
    citations: [{ source_note_id: NOTE, citation_key: "smith2024" }],
  };
}

function canvas() {
  return render(<GraphCanvas config={CONFIG} model={graphModel(graph())} />);
}

test("点に触れると、更新日時とタグを吹き出しで示す", () => {
  vi.useFakeTimers();
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
  act(() => vi.runAllTimers());
  expect(container.querySelector(".graph-detail")).toBeNull();
});

test("文献の点はcitation keyと題名を示す", () => {
  const { container } = canvas();
  const work = container.querySelector('.graph-vertex[data-kind="work"]');

  fireEvent.mouseEnter(work!);

  const detail = container.querySelector(".graph-detail");
  expect(detail?.textContent).toContain(WORK_TITLE);
  expect(detail?.textContent).toContain("smith2024");
  // 文献には更新日時が無いため、その行を出さない。
  expect(detail?.querySelector("time")).toBeNull();
  expect(detail?.textContent).toContain("なし");
});

test("図では文献の題名だけを常時表示しない", () => {
  const { container } = canvas();
  const note = container.querySelector('.graph-vertex[data-kind="note"]');
  const work = container.querySelector('.graph-vertex[data-kind="work"]');

  expect(note?.querySelector("text")?.textContent).toBe("先行研究の整理");
  expect(work?.querySelector("text")).toBeNull();
  expect(container.querySelector(".graph-detail")).toBeNull();
});

test("キーボードのフォーカスでも同じ吹き出しが出る", () => {
  const { container } = canvas();
  const note = container.querySelector('.graph-vertex[data-kind="note"]');
  const work = container.querySelector('.graph-vertex[data-kind="work"]');

  fireEvent.focus(note!);
  expect(container.querySelector(".graph-detail")).not.toBeNull();

  fireEvent.blur(note!);
  expect(container.querySelector(".graph-detail")).toBeNull();

  fireEvent.focus(work!);
  expect(container.querySelector(".graph-detail")?.textContent).toContain(
    WORK_TITLE,
  );
});

test("図の四隅にある点でも吹き出し全体を枠内へ寄せる", () => {
  vi.useFakeTimers();
  let point = rectangle({ left: 380, top: 55, width: 20, height: 20 });
  let panelHeight = 100;
  const measured = vi
    .spyOn(Element.prototype, "getBoundingClientRect")
    .mockImplementation(function (this: Element) {
      if (this.classList.contains("graph-stage")) {
        return rectangle({ left: 100, top: 50, width: 300, height: 200 });
      }
      if (this.classList.contains("graph-detail")) {
        return rectangle({ left: 0, top: 0, width: 240, height: panelHeight });
      }
      if (this.classList.contains("graph-vertex")) return point;
      return rectangle({ left: 0, top: 0, width: 0, height: 0 });
    });
  const { container } = canvas();
  const note = container.querySelector('.graph-vertex[data-kind="note"]');

  // 右上では点の下へ出し、右端へはみ出す分を左へ寄せる。
  fireEvent.mouseEnter(note!);
  expect(
    container.querySelector<HTMLElement>(".graph-detail")?.style,
  ).toMatchObject({ left: "52px", top: "33px" });

  // 左下では点の上へ出し、左端へはみ出す分を右へ寄せる。
  fireEvent.mouseLeave(note!);
  act(() => vi.runAllTimers());
  point = rectangle({ left: 100, top: 225, width: 20, height: 20 });
  fireEvent.mouseEnter(note!);
  expect(
    container.querySelector<HTMLElement>(".graph-detail")?.style,
  ).toMatchObject({ left: "8px", top: "67px" });

  // 高さが図本体に近い場合も、上下の余白を残せる位置まで寄せる。
  fireEvent.mouseLeave(note!);
  act(() => vi.runAllTimers());
  point = rectangle({ left: 240, top: 130, width: 20, height: 20 });
  panelHeight = 184;
  fireEvent.mouseEnter(note!);
  expect(
    container.querySelector<HTMLElement>(".graph-detail")?.style,
  ).toMatchObject({ top: "8px" });
  measured.mockRestore();
});

test("長い内容をスクロールするため点から吹き出しへマウスを移せる", () => {
  vi.useFakeTimers();
  const { container } = canvas();
  const note = container.querySelector('.graph-vertex[data-kind="note"]');

  fireEvent.mouseEnter(note!);
  const detail = container.querySelector(".graph-detail");
  fireEvent.mouseLeave(note!);
  fireEvent.mouseEnter(detail!);
  act(() => vi.runAllTimers());
  expect(container.querySelector(".graph-detail")).not.toBeNull();

  fireEvent.mouseLeave(detail!);
  act(() => vi.runAllTimers());
  expect(container.querySelector(".graph-detail")).toBeNull();
});

test("図の内容を更新したときは以前の点の吹き出しを残さない", () => {
  const first = graphModel(graph());
  const { container, rerender } = render(
    <GraphCanvas config={CONFIG} model={first} />,
  );
  const note = container.querySelector('.graph-vertex[data-kind="note"]');
  fireEvent.focus(note!);
  expect(container.querySelector(".graph-detail")).not.toBeNull();

  rerender(
    <GraphCanvas
      config={CONFIG}
      model={graphModel({
        notes: [],
        works: [],
        references: [],
        citations: [],
      })}
    />,
  );
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

  const work = screen.getByRole("link", { name: new RegExp(WORK_TITLE) });
  expect(work.getAttribute("aria-label")).toContain("citation key smith2024");
});
