import { expect, test } from "vitest";

import { NoteGraph } from "../src/api";
import { graphModel, workVertexId } from "../src/graph/model";
import { vertexHref } from "../src/graph/navigation";

const NOTE = "0197c9bc-0000-7000-8000-000000000001";
const OTHER = "0197c9bc-0000-7000-8000-000000000002";

function graph(): NoteGraph {
  return {
    notes: [
      {
        note_id: NOTE,
        title: "先行研究の整理",
        tags: ["研究"],
        updated_at_ms: 2,
      },
      { note_id: OTHER, title: "検証メモ", tags: [], updated_at_ms: 1 },
    ],
    works: [{ citation_key: "smith2024", title: "An Example Article" }],
    references: [{ source_note_id: NOTE, target_note_id: OTHER }],
    citations: [{ source_note_id: NOTE, citation_key: "smith2024" }],
  };
}

test("ノートと文献を同じ形の点として扱い、つながりの多い順に並べる", () => {
  const model = graphModel(graph());

  // つながりの多い順に並べ、同数のときは表示名で決める。
  expect(model.vertices.map((vertex) => vertex.label)).toEqual([
    "先行研究の整理",
    "An Example Article",
    "検証メモ",
  ]);
  expect(model.vertices[0].degree).toBe(2);
  expect(model.vertices[0].kind).toBe("note");
  expect(model.vertices[1].kind).toBe("work");
  expect(model.edges.map((edge) => edge.kind)).toEqual([
    "reference",
    "citation",
  ]);
});

test("片端の無い線は描かない", () => {
  const source = graph();
  source.references = [{ source_note_id: NOTE, target_note_id: "missing" }];
  source.citations = [{ source_note_id: "missing", citation_key: "smith2024" }];

  const model = graphModel(source);

  expect(model.edges).toEqual([]);
  // 線が消えても点の数は変わらない。存在しない相手を点として作らない。
  expect(model.vertices).toHaveLength(3);
});

test("題名の無い文献はcitation keyで示す", () => {
  const source = graph();
  source.works = [{ citation_key: "smith2024", title: null }];

  const model = graphModel(source);

  expect(
    model.vertices.find((vertex) => vertex.id === workVertexId("smith2024"))
      ?.label,
  ).toBe("smith2024");
});

test("点の移動先は、ノートが閲覧画面、文献が書誌ライブラリーである", () => {
  const config = {
    apiBase: "/api/v3",
    basePath: "/",
    path: "/graph",
    search: "",
    styleNonce: "test",
  };
  const model = graphModel(graph());
  const note = model.vertices.find((vertex) => vertex.kind === "note");
  const work = model.vertices.find((vertex) => vertex.kind === "work");

  expect(vertexHref(config, note!)).toBe(`/notes/${NOTE}`);
  expect(vertexHref(config, work!)).toBe("/bibliography?query=smith2024");
});
