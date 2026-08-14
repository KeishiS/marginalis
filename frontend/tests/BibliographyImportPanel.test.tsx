import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import { BibliographyImportPanel } from "../src/routes/BibliographyImportPanel";

const SOURCE_ID = "0197c9bc-0000-7000-8000-000000000101";
const ITEM_ID = "0197c9bc-0000-7000-8000-000000000102";

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

function jsonResponse(value: unknown, status = 200) {
  return new Response(JSON.stringify(value), {
    status,
    headers: { "content-type": "application/json" },
  });
}

function cslJsonFile(value: unknown, name = "library.json") {
  const file = new File([JSON.stringify(value)], name, {
    type: "application/json",
  });
  Object.defineProperty(file, "text", {
    value: async () => JSON.stringify(value),
  });
  return file;
}

test("未解決の競合を止め、選択した計画だけを適用する", async () => {
  const source = {
    source_id: SOURCE_ID,
    method: "csl_json_file",
    display_name: "Zotero研究ライブラリ",
    revision: 1,
    created_at_ms: 100,
    last_imported_at_ms: 100,
  };
  const fetchMock = vi
    .fn<typeof fetch>()
    .mockResolvedValueOnce(jsonResponse([]))
    .mockResolvedValueOnce(
      jsonResponse({
        source_id: null,
        source_revision: null,
        preview_token: "a".repeat(64),
        entries: [
          {
            position: 0,
            external_item_id: "external-1",
            citation_key: "smith2026",
            classification: "conflict",
            item_id: ITEM_ID,
            item_revision: 2,
            current_csl_json: {
              id: "smith2026",
              title: "Marginalis側の文献",
              type: "book",
            },
            candidates: [],
            rejection_code: null,
          },
          {
            position: 1,
            external_item_id: "external-2",
            citation_key: "jones2026",
            classification: "duplicate_candidate",
            item_id: null,
            item_revision: null,
            current_csl_json: null,
            candidates: [
              {
                item_id: ITEM_ID,
                citation_key: "existing2026",
                title: "既存文献",
                revision: 2,
                matched_by: ["doi"],
              },
            ],
            rejection_code: null,
          },
          {
            position: 2,
            external_item_id: null,
            citation_key: null,
            classification: "rejected",
            item_id: null,
            item_revision: null,
            current_csl_json: null,
            candidates: [],
            rejection_code: "item_not_object",
          },
        ],
      }),
    )
    .mockResolvedValueOnce(
      jsonResponse({
        source_id: SOURCE_ID,
        source_revision: 1,
        created: 0,
        updated: 1,
        kept: 1,
        excluded: 1,
      }),
    )
    .mockResolvedValueOnce(jsonResponse([source]));
  vi.stubGlobal("fetch", fetchMock);
  const onApplied = vi.fn().mockResolvedValue(undefined);

  render(<BibliographyImportPanel apiBase="/api/v3" onApplied={onApplied} />);
  fireEvent.click(
    screen.getByRole("button", { name: "CSL-JSONをまとめて取り込む" }),
  );
  await waitFor(() => expect(fetchMock).toHaveBeenCalledTimes(1));

  fireEvent.change(screen.getByLabelText("取込元の表示名"), {
    target: { value: "Zotero研究ライブラリ" },
  });
  const items = [
    { id: "external-1", title: "外部側の文献" },
    { id: "external-2", DOI: "10.1000/example" },
    "CSL-JSON項目ではない値",
  ];
  fireEvent.change(screen.getByLabelText("CSL-JSONファイル"), {
    target: { files: [cslJsonFile(items)] },
  });
  await screen.findByText("3件を読み込みました。事前確認を実行してください。");
  fireEvent.click(screen.getByRole("button", { name: "事前確認" }));

  const apply = await screen.findByRole("button", {
    name: "選択した計画を取り込む",
  });
  expect(apply).toBeDisabled();
  expect(screen.getByText(/existing2026.*既存文献/)).toBeTruthy();
  expect(
    screen.getByText("CSL-JSONの項目がオブジェクトではありません。"),
  ).toBeTruthy();
  expect(screen.getByText("Marginalis側の現在値")).toBeTruthy();
  expect(screen.getAllByText("外部側のCSL-JSON")).toHaveLength(3);

  fireEvent.change(screen.getByLabelText("1件目の処理"), {
    target: { value: "use_external" },
  });
  fireEvent.change(screen.getByLabelText("2件目の処理"), {
    target: { value: `link_existing_keep_local:${ITEM_ID}` },
  });
  expect(apply).toBeEnabled();
  fireEvent.click(apply);

  await screen.findByText(
    "取り込みました。新規0件、更新1件、保持1件、除外1件です。",
  );
  await waitFor(() => expect(fetchMock).toHaveBeenCalledTimes(4));
  expect(onApplied).toHaveBeenCalledOnce();

  expect(String(fetchMock.mock.calls[1][0])).toBe(
    "/api/v3/bibliography/import-previews",
  );
  const previewRequest = JSON.parse(
    String((fetchMock.mock.calls[1][1] as RequestInit).body),
  );
  expect(previewRequest).toEqual({
    source: { kind: "new", display_name: "Zotero研究ライブラリ" },
    items,
  });

  expect(String(fetchMock.mock.calls[2][0])).toBe(
    "/api/v3/bibliography/imports",
  );
  const applyRequest = JSON.parse(
    String((fetchMock.mock.calls[2][1] as RequestInit).body),
  );
  expect(applyRequest.preview_token).toBe("a".repeat(64));
  expect(applyRequest.decisions).toEqual([
    { position: 0, action: "use_external", candidate_item_id: null },
    {
      position: 1,
      action: "link_existing_keep_local",
      candidate_item_id: ITEM_ID,
    },
    { position: 2, action: "exclude", candidate_item_id: null },
  ]);
});

test("CSL-JSON項目配列でないファイルを事前確認へ進めない", async () => {
  vi.stubGlobal(
    "fetch",
    vi.fn<typeof fetch>().mockResolvedValue(jsonResponse([])),
  );
  render(<BibliographyImportPanel apiBase="/api/v3" onApplied={vi.fn()} />);
  fireEvent.click(
    screen.getByRole("button", { name: "CSL-JSONをまとめて取り込む" }),
  );
  await screen.findByRole("heading", { name: "CSL-JSONの一括取り込み" });
  fireEvent.change(screen.getByLabelText("CSL-JSONファイル"), {
    target: { files: [cslJsonFile({ id: "not-an-array" })] },
  });

  const alert = await screen.findByRole("alert");
  expect(alert).toHaveTextContent("CSL-JSON項目配列を選んでください");
  expect(screen.getByRole("button", { name: "事前確認" })).toBeDisabled();
});
