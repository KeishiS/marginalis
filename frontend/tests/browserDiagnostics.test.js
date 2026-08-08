// ブラウザー診断の分類が本文やtokenを漏らさないことを確認する単体試験。
// 分類の実装はtests/browser/fixtures/diagnostic-classification.jsにあります。
import { describe, expect, it } from "vitest";
import {
  browserDiagnostic,
  diagnosticSummary,
} from "../../tests/browser/fixtures/diagnostic-classification";

describe("ブラウザー診断の分類", () => {
  it("本文やtokenを含まない分類へ変換する", () => {
    const secret = "Bearer secret-token ノート本文";
    expect(
      diagnosticSummary(
        "console.error",
        `Refused to load a script because it violates the following directive: ${secret}`,
      ),
    ).toBe("Content Security Policy違反");
    expect(
      diagnosticSummary(
        "pageerror",
        `dynamic file 'double-struck' failed to load: ${secret}`,
      ),
    ).toBe("MathJax資源の読み込みまたは組版の失敗");

    const diagnostic = browserDiagnostic("console.error", secret, {
      url: "https://example.test/notes/private?token=secret-token",
      lineNumber: 12,
      columnNumber: 4,
    });
    expect(JSON.stringify(diagnostic)).not.toContain("secret-token");
    expect(JSON.stringify(diagnostic)).not.toContain("ノート本文");
    expect(diagnostic.source).toBe("private:12:4");
  });
});
