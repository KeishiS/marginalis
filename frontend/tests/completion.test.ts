import { afterEach, describe, expect, it, vi } from "vitest";
import { EditorState } from "@codemirror/state";
import { CompletionContext } from "@codemirror/autocomplete";

import {
  createTagCandidateLoader,
  tagCompletionSource,
} from "../src/editor/completion";

function contextAt(doc: string, pos = doc.length, explicit = false) {
  return new CompletionContext(EditorState.create({ doc }), pos, explicit);
}

const loadTags = () =>
  Promise.resolve(["machine-learning", "research", "rust"]);

describe("tagCompletionSource", () => {
  const source = tagCompletionSource(loadTags);

  it("タグ属性行で候補を返し、入力中の区画の先頭を補完位置にする", async () => {
    const doc = "= 題名\n:marginalis-tags: res";
    const result = await source(contextAt(doc));
    expect(result?.from).toBe(doc.length - "res".length);
    expect(result?.options.map((option) => option.label)).toEqual([
      "machine-learning",
      "research",
      "rust",
    ]);
  });

  it("2区画目以降も補完し、入力済みのタグを候補から除く", async () => {
    const doc = ":marginalis-tags: research, r";
    const result = await source(contextAt(doc));
    expect(result?.from).toBe(doc.length - "r".length);
    expect(result?.options.map((option) => option.label)).toEqual([
      "machine-learning",
      "rust",
    ]);
  });

  it("本文の行では何も返さない", async () => {
    expect(await source(contextAt("本文の res"))).toBeNull();
  });

  it("空の区画は明示的な要求のときだけ補完する", async () => {
    const doc = ":marginalis-tags: ";
    expect(await source(contextAt(doc, doc.length, false))).toBeNull();
    const explicit = await source(contextAt(doc, doc.length, true));
    expect(explicit?.options).toHaveLength(3);
  });

  it("属性名の内側にカーソルがある間は補完しない", async () => {
    const doc = ":marginalis-tags: rust";
    expect(await source(contextAt(doc, 5))).toBeNull();
  });
});

describe("createTagCandidateLoader", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  function notesResponse(tagsPerNote: string[][]) {
    return new Response(
      JSON.stringify(
        tagsPerNote.map((tags, index) => ({
          note_id: `0197c9bc-0000-7000-8000-${String(index).padStart(12, "0")}`,
          title: `ノート${index}`,
          tags,
          updated_at_ms: 1,
          revision: 1,
          created_via: "web",
          review_status: "pending",
          reviewed_revision: null,
          reviewed_at_ms: null,
          access: "manage",
        })),
      ),
      { status: 200, headers: { "content-type": "application/json" } },
    );
  }

  it("一覧のタグを重複なく整列して返し、2回目は取得し直さない", async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValue(notesResponse([["rust", "research"], ["research"]]));
    vi.stubGlobal("fetch", fetchMock);
    const load = createTagCandidateLoader("/api/v3");
    expect(await load()).toEqual(["research", "rust"]);
    expect(await load()).toEqual(["research", "rust"]);
    expect(fetchMock).toHaveBeenCalledTimes(1);
  });

  it("取得に失敗した場合は空の候補とし、次の要求で取得し直す", async () => {
    const fetchMock = vi
      .fn()
      .mockRejectedValueOnce(new Error("network"))
      .mockResolvedValue(notesResponse([["rust"]]));
    vi.stubGlobal("fetch", fetchMock);
    const load = createTagCandidateLoader("/api/v3");
    expect(await load()).toEqual([]);
    expect(await load()).toEqual(["rust"]);
    expect(fetchMock).toHaveBeenCalledTimes(2);
  });
});
