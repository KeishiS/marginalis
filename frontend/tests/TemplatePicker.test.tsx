import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import { TemplatePicker } from "../src/editor/TemplatePicker";

const TEMPLATE_ID = "0197c9bc-0000-7000-8000-000000000001";
const TEMPLATE_SOURCE = "= 実験記録\n\n== 目的\n\n== 結果\n";

function summary(noteId: string, title: string, tags: string[]) {
  return {
    note_id: noteId,
    title,
    tags,
    updated_at_ms: 1,
    revision: 1,
    created_via: "web",
    review_status: "pending",
    reviewed_revision: null,
    reviewed_at_ms: null,
    access: "manage",
  };
}

/** URLで応答を分けるfetch。一覧と単一ノートの両方の要求へ答える。 */
function stubRoutedFetch(templates: ReturnType<typeof summary>[]) {
  const fetchMock = vi.fn<typeof fetch>().mockImplementation((input) => {
    const url = String(input);
    const body = url.endsWith("/notes")
      ? templates
      : {
          note_id: TEMPLATE_ID,
          title: "実験記録の雛形",
          source: TEMPLATE_SOURCE,
          tags: ["テンプレート"],
          created_at_ms: 1,
          updated_at_ms: 1,
          revision: 1,
          created_via: "web",
          review_status: "pending",
          reviewed_revision: null,
          reviewed_at_ms: null,
        };
    return Promise.resolve(
      new Response(JSON.stringify(body), {
        status: 200,
        headers: { "content-type": "application/json" },
      }),
    );
  });
  vi.stubGlobal("fetch", fetchMock);
  return fetchMock;
}

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

test("テンプレートが無い場合は何も表示しない", async () => {
  stubRoutedFetch([summary(TEMPLATE_ID, "通常のノート", ["研究"])]);
  const { container } = render(
    <TemplatePicker
      apiBase="/api/v3"
      disabled={false}
      dirty={false}
      onApply={() => {}}
    />,
  );
  await vi.waitFor(() =>
    expect(screen.queryByText("テンプレートから開始")).not.toBeInTheDocument(),
  );
  expect(container).toBeEmptyDOMElement();
});

test("テンプレートを選ぶと本文が適用される", async () => {
  stubRoutedFetch([
    summary(TEMPLATE_ID, "実験記録の雛形", ["テンプレート"]),
    summary("0197c9bc-0000-7000-8000-000000000002", "通常のノート", ["研究"]),
  ]);
  const onApply = vi.fn();
  render(
    <TemplatePicker
      apiBase="/api/v3"
      disabled={false}
      dirty={false}
      onApply={onApply}
    />,
  );
  const picker = await screen.findByRole("combobox", {
    name: "テンプレートから開始",
  });
  expect(screen.queryByText("通常のノート")).not.toBeInTheDocument();
  fireEvent.change(picker, { target: { value: TEMPLATE_ID } });
  await vi.waitFor(() => expect(onApply).toHaveBeenCalledWith(TEMPLATE_SOURCE));
});

test("入力がある場合は置き換えの確認を挟む", async () => {
  stubRoutedFetch([summary(TEMPLATE_ID, "実験記録の雛形", ["テンプレート"])]);
  const onApply = vi.fn();
  render(
    <TemplatePicker
      apiBase="/api/v3"
      disabled={false}
      dirty
      onApply={onApply}
    />,
  );
  const picker = await screen.findByRole("combobox", {
    name: "テンプレートから開始",
  });

  fireEvent.change(picker, { target: { value: TEMPLATE_ID } });
  const dialog = await screen.findByRole("alertdialog");
  fireEvent.click(screen.getByRole("button", { name: "取り消す" }));
  expect(onApply).not.toHaveBeenCalled();
  expect(dialog).not.toBeInTheDocument();

  fireEvent.change(picker, { target: { value: TEMPLATE_ID } });
  await screen.findByRole("alertdialog");
  fireEvent.click(screen.getByRole("button", { name: "置き換える" }));
  await vi.waitFor(() => expect(onApply).toHaveBeenCalledWith(TEMPLATE_SOURCE));
});
