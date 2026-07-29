import { expect, test } from "vitest";

import {
  ASCII_DOC_COMMANDS,
  asciiDocCommandEdit,
} from "../src/asciiDocEditing";

test.each([
  ["title", "= 題名", "題名"],
  ["section", "== 節題", "節題"],
  ["list", "* 項目", "項目"],
  ["link", "https://example.com[リンク]", "https://example.com"],
  ["code-block", "[source,text]\n----\nコード\n----", "コード"],
  ["inline-math", "stem:[x]", "x"],
  ["block-math", "[latexmath]\n++++\nx\n++++", "x"],
  ["note-reference", "xref:note:NOTE_ID[参照]", "NOTE_ID"],
] as const)(
  "選択範囲がない場合に%sの入力例を挿入して置換箇所を選ぶ",
  (command, inserted, selected) => {
    const source = "前後";
    const edit = asciiDocCommandEdit(command, source, 1, 1);
    expect(edit.insert).toBe(inserted);
    const result =
      source.slice(0, edit.from) + edit.insert + source.slice(edit.to);
    expect(result.slice(edit.selection.anchor, edit.selection.head)).toBe(
      selected,
    );
  },
);

test.each(ASCII_DOC_COMMANDS)(
  "逆向きの選択範囲にも%sを一回の編集として適用する",
  (command) => {
    const source = "前選択後";
    const edit = asciiDocCommandEdit(command, source, 3, 1);
    expect(edit.from).toBe(1);
    expect(edit.to).toBe(3);
    expect(edit.insert).toContain("選択");
    expect(edit.selection.anchor).toBeGreaterThanOrEqual(edit.from);
    expect(edit.selection.head).toBeLessThanOrEqual(
      edit.from + edit.insert.length,
    );
  },
);

test("複数行の箇条書きは各行へ記号を付ける", () => {
  const source = "一\n二";
  const edit = asciiDocCommandEdit("list", source, 0, source.length);
  expect(edit.insert).toBe("* 一\n* 二");
  expect(edit.selection).toEqual({ anchor: 0, head: edit.insert.length });
});

test("選択した表示名をリンクへ残し、URLを置換対象にする", () => {
  const source = "詳しい説明";
  const edit = asciiDocCommandEdit("link", source, 0, source.length);
  expect(edit.insert).toBe("https://example.com[詳しい説明]");
  expect(edit.insert.slice(edit.selection.anchor, edit.selection.head)).toBe(
    "https://example.com",
  );
});
