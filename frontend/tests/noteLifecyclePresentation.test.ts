import { describe, expect, it } from "vitest";

import { ApiError } from "../src/api";
import {
  noteDeletionProblem,
  noteRestorationProblem,
} from "../src/noteLifecyclePresentation";

describe("ノート削除の問題表示", () => {
  it.each([
    [409, "画面を再読み込み"],
    [403, "一覧から開き直して"],
    [404, "一覧から開き直して"],
    [503, "時間を置いて"],
  ])("HTTP %iでは次の操作を示す", (status, expected) => {
    expect(
      noteDeletionProblem(
        new ApiError(status, {
          code: "unavailable",
          message: "internal detail",
        }),
      ),
    ).toContain(expected);
  });

  it("通信障害では接続の確認を案内する", () => {
    expect(noteDeletionProblem(new TypeError("Failed to fetch"))).toContain(
      "接続を確認",
    );
  });
});

describe("ノート復元の問題表示", () => {
  it.each([
    [409, "一覧を再読み込み"],
    [410, "復元期限を過ぎて"],
    [403, "権限を確認できません"],
    [404, "権限を確認できません"],
    [503, "時間を置いて"],
  ])("HTTP %iでは次の操作を示す", (status, expected) => {
    expect(
      noteRestorationProblem(
        new ApiError(status, { code: "unavailable", message: "detail" }),
      ),
    ).toContain(expected);
  });

  it("通信障害では接続の確認を案内する", () => {
    expect(noteRestorationProblem(new TypeError("Failed to fetch"))).toContain(
      "接続を確認",
    );
  });
});
