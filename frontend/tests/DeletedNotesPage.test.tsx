import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import { DeletedNotesPage } from "../src/routes/DeletedNotesPage";

const config = {
  apiBase: "/marginalis/api/v3",
  basePath: "/marginalis",
  path: "/notes/deleted",
  search: "?tag=research&page=2",
  styleNonce: "test-style-nonce",
};

afterEach(() => {
  cleanup();
  document.cookie = "__Host-marginalis_csrf=; Max-Age=0; path=/; Secure";
  vi.unstubAllGlobals();
});

test("削除済みノートの保持情報を表示し、確認後に復元する", async () => {
  document.cookie = "__Host-marginalis_csrf=test-csrf; path=/; Secure";
  const future = Date.now() + 2 * 24 * 60 * 60 * 1_000;
  const fetch = vi
    .fn()
    .mockResolvedValueOnce(
      new Response(
        JSON.stringify([
          {
            note_id: "0197c9bc-0000-7000-8000-000000000001",
            title: "復元するノート",
            deleted_at_ms: 1,
            purge_at_ms: future,
            revision: 4,
          },
          {
            note_id: "0197c9bc-0000-7000-8000-000000000002",
            title: "期限切れノート",
            deleted_at_ms: 1,
            purge_at_ms: 2,
            revision: 5,
          },
        ]),
        { status: 200, headers: { "content-type": "application/json" } },
      ),
    )
    .mockResolvedValueOnce(
      new Response(
        JSON.stringify({
          note_id: "0197c9bc-0000-7000-8000-000000000001",
          title: "復元するノート",
          source: "= 復元するノート\n",
          tags: [],
          created_at_ms: 1,
          updated_at_ms: 2,
          revision: 5,
          created_via: "web",
          review_status: "pending",
          reviewed_revision: null,
          reviewed_at_ms: null,
        }),
        { status: 200, headers: { "content-type": "application/json" } },
      ),
    );
  vi.stubGlobal("fetch", fetch);
  const navigate = vi.fn();

  render(<DeletedNotesPage config={config} navigate={navigate} />);

  expect(await screen.findByText("復元するノート")).toBeInTheDocument();
  expect(
    screen.getByRole("link", { name: "ノート一覧へ戻る" }),
  ).toHaveAttribute("href", "/marginalis/?tag=research&page=2");
  expect(screen.getByText("rev-4")).toBeInTheDocument();
  expect(screen.getByText("復元期限を過ぎています。")).toBeInTheDocument();
  const restoreButton = screen.getAllByRole("button", { name: /^復元$/ })[0];
  fireEvent.click(restoreButton);
  expect(screen.getByRole("alertdialog")).toHaveTextContent("復元するノート");
  expect(screen.getByRole("button", { name: "取り消す" })).toHaveFocus();
  fireEvent.click(screen.getByRole("button", { name: "復元する" }));

  await waitFor(() =>
    expect(navigate).toHaveBeenCalledWith(
      "/marginalis/?tag=research&notice=note-restored",
    ),
  );
  expect(fetch).toHaveBeenLastCalledWith(
    "/marginalis/api/v3/notes/0197c9bc-0000-7000-8000-000000000001/restore",
    expect.objectContaining({
      method: "POST",
      headers: expect.objectContaining({
        "if-match": '"rev-4"',
        "x-csrf-token": "test-csrf",
      }),
    }),
  );
});

test.each([
  {
    status: 409,
    code: "conflict",
    message:
      "別の操作でノートが更新されました。取り消して一覧を再読み込みしてから、もう一度復元してください。",
  },
  {
    status: 410,
    code: "retention_expired",
    message: "復元期限を過ぎています",
  },
])(
  "$status応答では一覧を残して再読み込みを案内する",
  async ({ status, code, message }) => {
    const future = Date.now() + 24 * 60 * 60 * 1_000;
    vi.stubGlobal(
      "fetch",
      vi
        .fn()
        .mockResolvedValueOnce(
          new Response(
            JSON.stringify([
              {
                note_id: "0197c9bc-0000-7000-8000-000000000001",
                title: "境界のノート",
                deleted_at_ms: 1,
                purge_at_ms: future,
                revision: 4,
              },
            ]),
            { status: 200, headers: { "content-type": "application/json" } },
          ),
        )
        .mockResolvedValueOnce(
          new Response(
            JSON.stringify({
              code,
              message: "restore failed",
            }),
            { status, headers: { "content-type": "application/json" } },
          ),
        ),
    );

    render(<DeletedNotesPage config={config} navigate={vi.fn()} />);
    fireEvent.click(await screen.findByRole("button", { name: /^復元$/ }));
    fireEvent.click(screen.getByRole("button", { name: "復元する" }));

    expect(await screen.findByRole("alert")).toHaveTextContent(message);
    // モーダル表示中の背面は支援技術から隠れるため、hiddenを含めて一覧の残存を確かめる。
    expect(
      screen.getByRole("heading", { name: "境界のノート", hidden: true }),
    ).toBeInTheDocument();
    expect(screen.getByRole("alertdialog")).toBeInTheDocument();
  },
);
