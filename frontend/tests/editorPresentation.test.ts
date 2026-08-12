import { expect, test } from "vitest";

import {
  diagnosticLocation,
  diagnosticMessage,
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

test("サーバーが返した行と列を表示する", () => {
  expect(
    diagnosticLocation({
      code: "asciidoc_parse_failed",
      severity: "error",
      target: { field: "source" },
      span: { start: 10, end: 13, unit: "utf8_byte" },
      position: { line: 3, column: 1 },
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
  expect(
    problemMessage({ code: "advisories_rejected", message: "internal detail" }),
  ).toBe("警告を解消してから保存してください。");
});

test("行の長さ超過は上限と対処を示す", () => {
  expect(
    diagnosticMessage(
      "line-too-long",
      "line has 128 characters; maximum is 100",
    ),
  ).toBe(
    "1行は最大100文字です。この行は128文字あります。内容の区切りで改行してください。",
  );
});
