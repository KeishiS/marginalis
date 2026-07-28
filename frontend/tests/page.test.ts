import { expect, test } from "vitest";

import { localizeTimes } from "../src/page";

test("RFC 3339日時を利用者のタイムゾーンに合わせて表示する", () => {
  document.body.innerHTML =
    '<time data-local-time datetime="2026-07-28T06:00:00Z">2026-07-28T06:00:00Z</time>';

  localizeTimes(document);

  const time = document.querySelector("time");
  expect(time?.textContent).not.toBe("2026-07-28T06:00:00Z");
  expect(time?.getAttribute("datetime")).toBe("2026-07-28T06:00:00Z");
});

test("不正な日時は書き換えない", () => {
  document.body.innerHTML =
    '<time data-local-time datetime="invalid">表示できません</time>';

  localizeTimes(document);

  expect(document.querySelector("time")?.textContent).toBe("表示できません");
});
