import { expect, test } from "vitest";

import { noteRetentionStatus } from "../src/noteRetention";

const day = 24 * 60 * 60 * 1_000;

test("復元期限までの日数を端数切り上げで示す", () => {
  expect(noteRetentionStatus(2 * day, 1).label).toBe("復元期限まで2日です。");
  expect(noteRetentionStatus(day, day).label).toBe("本日まで復元できます。");
});

test("期限を過ぎたノートを復元不可として示す", () => {
  expect(noteRetentionStatus(day, day + 1)).toEqual({
    expired: true,
    label: "復元期限を過ぎています。",
  });
});
