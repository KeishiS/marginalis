import { expect, test } from "vitest";

import { alignThreeVersions } from "../src/editorConflict";

test("編集開始時点、編集中、現在保存済みの行を対応付ける", () => {
  const rows = alignThreeVersions(
    "共通\n削除対象\n末尾",
    "共通\n編集中の追加\n末尾",
    "共通\n現在の追加\n削除対象\n末尾",
  );

  expect(rows).toEqual(
    expect.arrayContaining([
      expect.objectContaining({
        line: "追加",
        editing: "編集中の追加",
        current: "現在の追加",
      }),
      expect.objectContaining({
        line: "2",
        status: "編集中から削除",
        editingStarted: "削除対象",
      }),
    ]),
  );
});

test("大きな文書ではメモリ量を制限した行番号対応へ切り替える", () => {
  const baseline = Array.from({ length: 501 }, (_, index) => `行${index}`);
  const editing = [...baseline];
  editing[250] = "変更後";

  const rows = alignThreeVersions(
    baseline.join("\n"),
    editing.join("\n"),
    baseline.join("\n"),
  );

  expect(rows).toEqual(
    expect.arrayContaining([
      expect.objectContaining({
        line: "追加",
        editing: "変更後",
        current: null,
      }),
      expect.objectContaining({
        line: "251",
        status: "編集中から削除",
      }),
    ]),
  );
});
