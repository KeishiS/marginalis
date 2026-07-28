import { expect, test } from "vitest";

import {
  diagnosticLocation,
  editorStatus,
  problemMessage,
} from "../src/editorPresentation";
import { externalPath } from "../src/paths";

test("保存状態は利用者が次に取る操作を優先して示す", () => {
  expect(
    editorStatus({
      saving: false,
      isDirty: true,
      failed: true,
      conflicted: true,
      notice: "保存しました。",
    }),
  ).toBe("更新内容の競合を解消してください。");
});

test("UTF-8 byte位置を行と列へ変換する", () => {
  const source = "= 題名\n\n日本語";
  const start = new TextEncoder().encode("= 題名\n\n").length;

  expect(
    diagnosticLocation(source, {
      code: "asciidoc_parse_failed",
      target: { field: "source" },
      span: { start, end: start + 3, unit: "utf8_byte" },
      message: "invalid",
    }),
  ).toBe("3行1列: ");
});

test("公開経路と問題表示を一か所で正規化する", () => {
  expect(externalPath("/marginalis/", "/notes/id")).toBe(
    "/marginalis/notes/id",
  );
  expect(
    problemMessage({ code: "validation_failed", message: "internal detail" }),
  ).toBe("入力内容を確認してください。");
});
