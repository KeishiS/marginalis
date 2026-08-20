import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { NoteHistoryPage } from "../src/routes/NoteHistoryPage";

const noteId = "0197c9bc-0000-7000-8000-000000000001";
const config = {
  apiBase: "/api/v3",
  basePath: "/",
  path: `/notes/${noteId}/history`,
  search: "",
  styleNonce: "test-nonce",
};

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

describe("NoteHistoryPage", () => {
  it("過去版の原文と行単位diffを表示する", async () => {
    vi.stubGlobal("fetch", historyFetch(false));
    render(<NoteHistoryPage config={config} noteId={noteId} />);

    expect(
      await screen.findByText("最初の本文", { exact: false }),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "差分を表示" }));
    expect(await screen.findByLabelText("行単位の差分")).toHaveTextContent(
      "-最初の本文",
    );
    expect(screen.getByLabelText("行単位の差分")).toHaveTextContent(
      "+現在の本文",
    );
    expect(
      screen.getByRole("button", { name: "この版を復元" }),
    ).toBeInTheDocument();
  });

  it("削除中も所有者は履歴を参照できるが本文復元は表示しない", async () => {
    vi.stubGlobal("fetch", historyFetch(true));
    render(<NoteHistoryPage config={config} noteId={noteId} />);

    expect(
      await screen.findByText("最初の本文", { exact: false }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "この版を復元" }),
    ).not.toBeInTheDocument();
  });
});

function historyFetch(deleted: boolean) {
  return vi.fn(async (input: RequestInfo | URL) => {
    const url = String(input);
    if (url.endsWith("/history")) {
      return jsonResponse([
        revisionSummary(2, "content_updated", 200),
        revisionSummary(1, "created", 100),
      ]);
    }
    if (url.includes("history-diff")) {
      return jsonResponse({
        from_revision: 1,
        to_revision: 2,
        unified_diff:
          "--- revision-1\n+++ revision-2\n@@ -1 +1 @@\n-最初の本文\n+現在の本文\n",
      });
    }
    if (url.endsWith("/history/1")) {
      return jsonResponse(revisionResponse(1, "最初の本文", null));
    }
    if (url.endsWith("/history/2")) {
      return jsonResponse(
        revisionResponse(2, "現在の本文", deleted ? 200 : null),
      );
    }
    throw new Error(`unexpected request: ${url}`);
  });
}

function revisionSummary(revision: number, kind: string, changedAt: number) {
  return {
    revision,
    changed_at_ms: changedAt,
    changed_by_issuer: "https://id.example.test",
    changed_by_subject: "alice",
    kind,
  };
}

function revisionResponse(
  revision: number,
  body: string,
  deletedAt: number | null,
) {
  return {
    note: {
      note_id: noteId,
      title: "履歴の試験",
      source: `= 履歴の試験\n\n${body}\n`,
      tags: [],
      created_at_ms: 100,
      updated_at_ms: revision * 100,
      revision,
      created_via: "web",
      review_status: "pending",
      reviewed_revision: null,
      reviewed_at_ms: null,
    },
    access: "manage",
    deleted_at_ms: deletedAt,
    changed_by_issuer: "https://id.example.test",
    changed_by_subject: "alice",
    kind: revision === 1 ? "created" : "content_updated",
  };
}

function jsonResponse(value: unknown) {
  return new Response(JSON.stringify(value), {
    status: 200,
    headers: { "content-type": "application/json" },
  });
}
