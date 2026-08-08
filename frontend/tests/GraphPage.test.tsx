import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import { GraphPage } from "../src/routes/GraphPage";

const NOTE = "0197c9bc-0000-7000-8000-000000000001";

const CONFIG = {
  apiBase: "/api/v3",
  basePath: "/",
  path: "/graph",
  search: "",
  styleNonce: "test-nonce",
};

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
  window.history.replaceState(null, "", "/");
});

function graphResponse() {
  return new Response(
    JSON.stringify({
      notes: [
        { note_id: NOTE, title: "先行研究の整理", tags: [], updated_at_ms: 1 },
      ],
      works: [],
      references: [],
      citations: [],
    }),
    { status: 200, headers: { "content-type": "application/json" } },
  );
}

/** 呼ばれたURLを覚えるfetch。図の要求は読み取りだけなので本文は見ない。 */
function recordingFetch(): { urls: string[]; fetch: typeof fetch } {
  const urls: string[] = [];
  const stub = vi.fn(async (input: RequestInfo | URL) => {
    urls.push(String(input));
    return graphResponse();
  });
  vi.stubGlobal("fetch", stub);
  return { urls, fetch: stub as unknown as typeof fetch };
}

test("起点を指定しない場合は範囲の指定を送らない", async () => {
  const { urls } = recordingFetch();

  render(<GraphPage config={CONFIG} />);

  await waitFor(() => expect(urls.length).toBe(1));
  expect(urls[0]).toBe("/api/v3/notes/graph");
  expect(screen.queryByText("辿る階層")).toBeNull();
});

test("URLの起点と階層を読み、そのまま要求へ渡す", async () => {
  const { urls } = recordingFetch();

  const { container } = render(
    <GraphPage config={{ ...CONFIG, search: `?origin=${NOTE}&depth=3` }} />,
  );

  await waitFor(() => expect(urls.length).toBe(1));
  expect(urls[0]).toBe(`/api/v3/notes/graph?origin=${NOTE}&depth=3`);
  // 起点にしたノートの題名を、応答が返った図から引いて示す。題名は図と一覧にも出るため、
  // 帯の中だけを見る。
  await waitFor(() =>
    expect(container.querySelector(".graph-origin strong")?.textContent).toBe(
      "先行研究の整理",
    ),
  );
});

test("URLの検索語を入力と要求へ復元する", async () => {
  const { urls } = recordingFetch();

  render(<GraphPage config={{ ...CONFIG, search: "?query=Rust" }} />);

  await waitFor(() => expect(urls.length).toBe(1));
  expect(urls[0]).toBe("/api/v3/notes/graph?query=Rust");
  expect(screen.getByLabelText("語で絞り込む")).toHaveValue("Rust");
});

test("上限を超える階層は既定の1として扱う", async () => {
  const { urls } = recordingFetch();

  render(
    <GraphPage config={{ ...CONFIG, search: `?origin=${NOTE}&depth=99` }} />,
  );

  await waitFor(() => expect(urls.length).toBe(1));
  expect(urls[0]).toBe(`/api/v3/notes/graph?origin=${NOTE}&depth=1`);
});

test("階層を選び直すと読み直し、全体へ戻せる", async () => {
  const { urls } = recordingFetch();

  render(
    <GraphPage config={{ ...CONFIG, search: `?origin=${NOTE}&depth=1` }} />,
  );
  await waitFor(() => expect(urls.length).toBe(1));

  fireEvent.change(await screen.findByLabelText("辿る階層"), {
    target: { value: "4" },
  });
  await waitFor(() => expect(urls.length).toBe(2));
  expect(urls[1]).toBe(`/api/v3/notes/graph?origin=${NOTE}&depth=4`);
  expect(window.location.search).toBe(`?origin=${NOTE}&depth=4`);

  fireEvent.click(screen.getByRole("button", { name: "全体を見る" }));
  await waitFor(() => expect(urls.length).toBe(3));
  expect(urls[2]).toBe("/api/v3/notes/graph");
  expect(window.location.search).toBe("");
  expect(screen.queryByText("辿る階層")).toBeNull();
});

test("検索条件の変更をURLへ反映する", async () => {
  const { urls } = recordingFetch();
  render(<GraphPage config={CONFIG} />);
  await waitFor(() => expect(urls.length).toBe(1));

  fireEvent.change(screen.getByLabelText("語で絞り込む"), {
    target: { value: "日本語 検索" },
  });
  fireEvent.click(screen.getByRole("button", { name: "絞り込む" }));

  await waitFor(() => expect(urls.length).toBe(2));
  expect(window.location.search).toBe(
    "?query=%E6%97%A5%E6%9C%AC%E8%AA%9E+%E6%A4%9C%E7%B4%A2",
  );
});
