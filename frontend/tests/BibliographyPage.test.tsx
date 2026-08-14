import {
  cleanup,
  fireEvent,
  render,
  screen,
  within,
  waitFor,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import { BibliographyPage } from "../src/routes/BibliographyPage";

const CONFIG = {
  apiBase: "/api/v3",
  basePath: "/",
  path: "/bibliography",
  search: "",
  styleNonce: "test-nonce",
};

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

function libraryResponse() {
  return new Response(
    JSON.stringify([
      {
        item_id: "0197c9bc-0000-7000-8000-0000000000a1",
        citation_key: "smith2024",
        csl_json: { id: "smith2024", type: "book", title: "An Example" },
        created_at_ms: 1,
        updated_at_ms: 2,
        revision: 1,
      },
      {
        item_id: "0197c9bc-0000-7000-8000-0000000000a2",
        citation_key: "tanaka2025",
        csl_json: { id: "tanaka2025", type: "book", title: "別の文献" },
        created_at_ms: 1,
        updated_at_ms: 2,
        revision: 1,
      },
    ]),
    { status: 200, headers: { "content-type": "application/json" } },
  );
}

test("文献カードの操作を情報部分と分けて並べる", async () => {
  vi.stubGlobal("fetch", vi.fn().mockResolvedValue(libraryResponse()));

  render(<BibliographyPage config={CONFIG} />);

  const card = await waitFor(() =>
    screen.getByRole("button", { name: /smith2024/ }),
  );
  const remove = screen.getAllByRole("button", { name: "削除" })[0];

  // 情報部分は編集を始める操作、削除はその外側に置く。入れ子のボタンを作らない。
  expect(card.tagName).toBe("BUTTON");
  expect(card.contains(remove)).toBe(false);
  expect(card.parentElement).toBe(remove.parentElement?.parentElement);
});

test("未保存の編集を確認してから別のカードへ切り替える", async () => {
  vi.stubGlobal("fetch", vi.fn().mockResolvedValue(libraryResponse()));

  render(<BibliographyPage config={CONFIG} />);

  const first = await waitFor(() =>
    screen.getByRole("button", { name: /smith2024/ }),
  );
  const second = screen.getByRole("button", { name: /tanaka2025/ });
  const input = screen.getByLabelText("CSL-JSON");

  expect(first).toHaveAttribute("aria-current", "false");
  fireEvent.click(first);
  expect(first).toHaveAttribute("aria-current", "true");
  expect(screen.getByRole("button", { name: "更新" })).toBeTruthy();
  expect((input as HTMLTextAreaElement).value).toContain("smith2024");

  fireEvent.change(input, { target: { value: '{"id":"draft"}' } });
  fireEvent.click(second);
  expect(screen.getByRole("alertdialog")).toHaveTextContent(
    "編集中の内容を破棄しますか",
  );
  fireEvent.click(screen.getByRole("button", { name: "取り消す" }));
  expect(first).toHaveAttribute("aria-current", "true");
  expect(input).toHaveValue('{"id":"draft"}');

  fireEvent.click(second);
  fireEvent.click(screen.getByRole("button", { name: "変更を破棄" }));
  expect(second).toHaveAttribute("aria-current", "true");
  expect(first).toHaveAttribute("aria-current", "false");
  expect((input as HTMLTextAreaElement).value).toContain("tanaka2025");

  // 選び直しでは読み込み直さない。押し間違いで編集中の内容を失わせない。
  fireEvent.change(input, {
    target: { value: '{"id":"tanaka2025","note":1}' },
  });
  fireEvent.click(second);
  expect((input as HTMLTextAreaElement).value).toContain('"note":1');
});

test("主要な操作と取り消しにくい操作を色で区別する", async () => {
  vi.stubGlobal("fetch", vi.fn().mockResolvedValue(libraryResponse()));

  render(<BibliographyPage config={CONFIG} />);

  await waitFor(() => screen.getByRole("button", { name: /smith2024/ }));

  // 主要な操作はアクセント色、取り消しにくい操作は警告色を使う。
  expect(screen.getByRole("button", { name: "登録" })).toHaveAttribute(
    "data-variant",
    "default",
  );
  expect(screen.getAllByRole("button", { name: "削除" })[0]).toHaveAttribute(
    "data-variant",
    "destructive",
  );
  // 絞り込みは補助操作であり、アクセント色を重ねない。
  expect(screen.getByRole("button", { name: "検索" })).not.toHaveAttribute(
    "data-variant",
    "default",
  );
});

test("成功の知らせと失敗を、役割と見た目で区別する", async () => {
  vi.stubGlobal(
    "fetch",
    vi
      .fn()
      .mockResolvedValueOnce(libraryResponse())
      .mockResolvedValueOnce(new Response("", { status: 503 })),
  );

  render(<BibliographyPage config={CONFIG} />);

  const card = await waitFor(() =>
    screen.getByRole("button", { name: /smith2024/ }),
  );

  // 成功の知らせは待つだけでよいため、控えめに伝える。
  fireEvent.click(card);
  const notice = screen.getByRole("status");
  expect(notice.textContent).toContain("編集中です");
  expect(notice).toHaveAttribute("data-slot", "status-message");

  // 失敗は利用者の対応が要るため、割り込んで伝える。
  fireEvent.click(screen.getAllByRole("button", { name: "削除" })[0]);
  expect(screen.getByRole("alertdialog")).toHaveTextContent(
    "文献情報の削除は取り消せません",
  );
  fireEvent.click(screen.getByRole("button", { name: "削除する" }));
  const problem = await waitFor(() => screen.getByRole("alert"));
  expect(problem.textContent).toContain("削除できませんでした");
  expect(problem).toHaveAttribute("data-slot", "alert");
});

test("登録失敗を後から開いた削除確認へ混ぜない", async () => {
  vi.stubGlobal(
    "fetch",
    vi
      .fn()
      .mockResolvedValueOnce(libraryResponse())
      .mockResolvedValueOnce(new Response("", { status: 503 })),
  );
  render(<BibliographyPage config={CONFIG} />);
  await screen.findByRole("button", { name: /smith2024/ });

  fireEvent.click(screen.getByRole("button", { name: "登録" }));
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "登録できませんでした",
  );
  fireEvent.click(screen.getAllByRole("button", { name: "削除" })[0]);

  const dialog = screen.getByRole("alertdialog");
  expect(within(dialog).queryByRole("alert")).toBeNull();
  fireEvent.click(within(dialog).getByRole("button", { name: "取り消す" }));
  expect(
    screen.getByText("登録できませんでした", { exact: false }),
  ).toBeVisible();
});

test("未保存のCSL-JSONがある場合だけ画面離脱を警告する", async () => {
  vi.stubGlobal("fetch", vi.fn().mockResolvedValue(libraryResponse()));
  render(<BibliographyPage config={CONFIG} />);
  await screen.findByRole("button", { name: /smith2024/ });

  const cleanEvent = new Event("beforeunload", { cancelable: true });
  window.dispatchEvent(cleanEvent);
  expect(cleanEvent.defaultPrevented).toBe(false);

  fireEvent.change(screen.getByLabelText("CSL-JSON"), {
    target: { value: '{"id":"draft"}' },
  });
  const dirtyEvent = new Event("beforeunload", { cancelable: true });
  window.dispatchEvent(dirtyEvent);
  expect(dirtyEvent.defaultPrevented).toBe(true);
});

test("URLのqueryを初期の絞り込み条件として読む", async () => {
  const fetchMock = vi.fn().mockResolvedValue(libraryResponse());
  vi.stubGlobal("fetch", fetchMock);

  render(
    <BibliographyPage config={{ ...CONFIG, search: "?query=smith2024" }} />,
  );

  await waitFor(() => screen.getByRole("button", { name: /smith2024/ }));
  // グラフビューから文献を選んだ場合に、その項目へ絞った状態で開く。
  expect(String(fetchMock.mock.calls[0][0])).toContain("query=smith2024");
  expect((screen.getByLabelText("文献を検索") as HTMLInputElement).value).toBe(
    "smith2024",
  );
});

test("登録後の一覧再読込でも成功通知を保持する", async () => {
  const fetchMock = vi
    .fn()
    .mockResolvedValueOnce(libraryResponse())
    .mockResolvedValueOnce(
      new Response(
        JSON.stringify({
          item_id: "0197c9bc-0000-7000-8000-0000000000a3",
          citation_key: "smith2024",
          csl_json: { id: "smith2024", type: "article-journal" },
          created_at_ms: 1,
          updated_at_ms: 1,
          revision: 1,
        }),
        { status: 201 },
      ),
    )
    .mockResolvedValueOnce(libraryResponse());
  vi.stubGlobal("fetch", fetchMock);
  render(<BibliographyPage config={CONFIG} />);
  await waitFor(() => screen.getByRole("button", { name: /smith2024/ }));

  fireEvent.click(screen.getByRole("button", { name: "登録" }));

  expect(await screen.findByText("文献情報を登録しました。")).toHaveAttribute(
    "role",
    "status",
  );
  await waitFor(() => expect(fetchMock).toHaveBeenCalledTimes(3));
  expect(screen.getByText("文献情報を登録しました。")).toBeInTheDocument();
});

test("遅れて完了した古い検索結果を表示しない", async () => {
  let resolveOld!: (response: Response) => void;
  let resolveNew!: (response: Response) => void;
  const oldResponse = new Promise<Response>((resolve) => {
    resolveOld = resolve;
  });
  const newResponse = new Promise<Response>((resolve) => {
    resolveNew = resolve;
  });
  const fetchMock = vi
    .fn()
    .mockResolvedValueOnce(libraryResponse())
    .mockReturnValueOnce(oldResponse)
    .mockReturnValueOnce(newResponse);
  vi.stubGlobal("fetch", fetchMock);
  render(<BibliographyPage config={CONFIG} />);
  await waitFor(() => screen.getByRole("button", { name: /smith2024/ }));

  const input = screen.getByLabelText("文献を検索");
  const form = input.closest("form");
  fireEvent.change(input, { target: { value: "古い検索" } });
  fireEvent.submit(form!);
  fireEvent.change(input, { target: { value: "新しい検索" } });
  fireEvent.submit(form!);

  resolveNew(
    new Response(
      JSON.stringify([
        {
          item_id: "0197c9bc-0000-7000-8000-0000000000b1",
          citation_key: "new-result",
          csl_json: { id: "new-result", type: "book" },
          created_at_ms: 1,
          updated_at_ms: 1,
          revision: 1,
        },
      ]),
      { status: 200 },
    ),
  );
  await screen.findByRole("button", { name: /new-result/ });
  resolveOld(libraryResponse());
  await Promise.resolve();
  expect(
    screen.getByRole("button", { name: /new-result/ }),
  ).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: /smith2024/ })).toBeNull();
});

test("文献情報の変更中は検索条件を切り替えない", async () => {
  let completeMutation!: (response: Response) => void;
  const mutation = new Promise<Response>((resolve) => {
    completeMutation = resolve;
  });
  const fetchMock = vi
    .fn()
    .mockResolvedValueOnce(libraryResponse())
    .mockReturnValueOnce(mutation)
    .mockResolvedValueOnce(libraryResponse());
  vi.stubGlobal("fetch", fetchMock);
  render(<BibliographyPage config={CONFIG} />);
  await screen.findByRole("button", { name: /smith2024/ });

  fireEvent.click(screen.getByRole("button", { name: "登録" }));
  expect(screen.getByLabelText("文献を検索")).toBeDisabled();
  expect(screen.getByRole("button", { name: "検索" })).toBeDisabled();

  completeMutation(
    new Response(
      JSON.stringify({
        item_id: "0197c9bc-0000-7000-8000-0000000000a3",
        citation_key: "smith2024",
        csl_json: { id: "smith2024", type: "article-journal" },
        created_at_ms: 1,
        updated_at_ms: 1,
        revision: 1,
      }),
      { status: 201 },
    ),
  );
  await waitFor(() => expect(fetchMock).toHaveBeenCalledTimes(3));
  await waitFor(() =>
    expect(screen.getByLabelText("文献を検索")).toBeEnabled(),
  );
});

test("画面を離れたときに進行中の検索を中止する", async () => {
  const signals: AbortSignal[] = [];
  const fetchMock = vi.fn((_url: string, init?: RequestInit) => {
    signals.push(init?.signal as AbortSignal);
    if (signals.length === 1) return Promise.resolve(libraryResponse());
    return new Promise<Response>(() => undefined);
  });
  vi.stubGlobal("fetch", fetchMock);
  const { unmount } = render(<BibliographyPage config={CONFIG} />);
  await screen.findByRole("button", { name: /smith2024/ });

  fireEvent.submit(screen.getByLabelText("文献を検索").closest("form")!);
  await waitFor(() => expect(signals).toHaveLength(2));
  unmount();
  expect(signals[1].aborted).toBe(true);
});
