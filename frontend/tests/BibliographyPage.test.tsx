import { cleanup, render, screen, waitFor } from "@testing-library/react";
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
    ]),
    { status: 200, headers: { "content-type": "application/json" } },
  );
}

test("文献カードの編集と削除を一つのまとまりとして並べる", async () => {
  vi.stubGlobal("fetch", vi.fn().mockResolvedValue(libraryResponse()));

  render(<BibliographyPage config={CONFIG} />);

  const edit = await waitFor(() =>
    screen.getByRole("button", { name: "編集" }),
  );
  const remove = screen.getByRole("button", { name: "削除" });

  // 二つの操作が同じ親を持たないと、`space-between`が操作の間へも余白を配る。
  expect(edit.parentElement).not.toBeNull();
  expect(edit.parentElement).toBe(remove.parentElement);
  expect(edit.parentElement?.className).toBe("bibliography-item-actions");
  expect(edit.parentElement?.parentElement?.tagName).toBe("LI");
});
