import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import { BibliographyPage } from "../src/routes/BibliographyPage";

const CONFIG = {
  apiBase: "/api/v3",
  basePath: "/",
  path: "/bibliography",
  search: "",
  styleNonce: "test-nonce",
};

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

function libraryResponse() {
  return new Response(
    JSON.stringify([
      {
        item_id: "0197c9bc-0000-7000-8000-0000000000a1",
        citation_key: "smith2024",
        csl_json: { id: "smith2024", type: "book", title: "An Example" },
        created_at_ms: 1,
        updated_at_ms: 2,
        revision: 1,
      },
      {
        item_id: "0197c9bc-0000-7000-8000-0000000000a2",
        citation_key: "tanaka2025",
        csl_json: { id: "tanaka2025", type: "book", title: "別の文献" },
        created_at_ms: 1,
        updated_at_ms: 2,
        revision: 1,
      },
    ]),
    { status: 200, headers: { "content-type": "application/json" } },
  );
}

test("文献カードの操作を情報部分と分けて並べる", async () => {
  vi.stubGlobal("fetch", vi.fn().mockResolvedValue(libraryResponse()));

  render(<BibliographyPage config={CONFIG} />);

  const card = await waitFor(() =>
    screen.getByRole("button", { name: /smith2024/ }),
  );
  const remove = screen.getAllByRole("button", { name: "削除" })[0];

  // 情報部分は編集を始める操作、削除はその外側に置く。入れ子のボタンを作らない。
  expect(card.tagName).toBe("BUTTON");
  expect(card.contains(remove)).toBe(false);
  expect(remove.parentElement?.className).toBe("bibliography-item-actions");
  expect(card.parentElement).toBe(remove.parentElement?.parentElement);
});

test("カードを選ぶと編集中になり、別のカードへ確認なしで切り替わる", async () => {
  vi.stubGlobal("fetch", vi.fn().mockResolvedValue(libraryResponse()));

  render(<BibliographyPage config={CONFIG} />);

  const first = await waitFor(() =>
    screen.getByRole("button", { name: /smith2024/ }),
  );
  const second = screen.getByRole("button", { name: /tanaka2025/ });
  const input = screen.getByLabelText("CSL-JSON");

  expect(first).toHaveAttribute("aria-current", "false");
  fireEvent.click(first);
  expect(first).toHaveAttribute("aria-current", "true");
  expect(screen.getByRole("button", { name: "更新" })).toBeTruthy();
  expect((input as HTMLTextAreaElement).value).toContain("smith2024");

  // 未保存のまま書き換えても、別のカードを選べば確認なしで切り替わる。
  fireEvent.change(input, { target: { value: '{"id":"draft"}' } });
  fireEvent.click(second);
  expect(second).toHaveAttribute("aria-current", "true");
  expect(first).toHaveAttribute("aria-current", "false");
  expect((input as HTMLTextAreaElement).value).toContain("tanaka2025");

  // 選び直しでは読み込み直さない。押し間違いで編集中の内容を失わせない。
  fireEvent.change(input, {
    target: { value: '{"id":"tanaka2025","note":1}' },
  });
  fireEvent.click(second);
  expect((input as HTMLTextAreaElement).value).toContain('"note":1');
});

test("主要な操作と取り消しにくい操作を色で区別する", async () => {
  vi.stubGlobal("fetch", vi.fn().mockResolvedValue(libraryResponse()));

  render(<BibliographyPage config={CONFIG} />);

  await waitFor(() => screen.getByRole("button", { name: /smith2024/ }));

  // 主要な操作はアクセント色、取り消しにくい操作は警告色を使う。
  expect(screen.getByRole("button", { name: "登録" }).className).toContain(
    "button-primary",
  );
  expect(
    screen.getAllByRole("button", { name: "削除" })[0].className,
  ).toContain("button-danger");
  // 絞り込みは補助操作であり、アクセント色を重ねない。
  expect(screen.getByRole("button", { name: "検索" }).className).not.toContain(
    "button-primary",
  );
});
