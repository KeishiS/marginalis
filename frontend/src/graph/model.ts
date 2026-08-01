import { NoteGraph } from "../api";

/** 図に置く点。ノートと文献の両方を同じ形で扱う。 */
export interface GraphVertex {
  id: string;
  kind: "note" | "work";
  label: string;
  /** ノートのタグ。文献には無い。 */
  tags: string[];
  /** つながっている線の数。大きさと並び順に使う。 */
  degree: number;
  /** ノートの最終更新時刻。文献には無いため`null`とする。 */
  updatedAtMs: number | null;
  /** 文献のcitation key。ノートには無いため`null`とする。 */
  citationKey: string | null;
}

/** 図に引く線。 */
export interface GraphEdge {
  id: string;
  kind: "reference" | "citation";
  source: string;
  target: string;
}

export interface GraphModel {
  vertices: GraphVertex[];
  edges: GraphEdge[];
}

/** 文献の点を、ノートのnote IDと衝突しない識別子にする。 */
export function workVertexId(citationKey: string): string {
  return `work:${citationKey}`;
}

/**
 * 公開契約の形を、図が扱う点と線へ直す。
 *
 * 線は両端が点として存在するものだけ残す。サーバーは可視な範囲だけを返すが、画面側でも
 * 同じ条件を保ち、片端の無い線を描かない。
 */
export function graphModel(graph: NoteGraph): GraphModel {
  const vertices = new Map<string, GraphVertex>();
  for (const note of graph.notes) {
    vertices.set(note.note_id, {
      id: note.note_id,
      kind: "note",
      label: note.title,
      tags: note.tags,
      degree: 0,
      updatedAtMs: note.updated_at_ms,
      citationKey: null,
    });
  }
  for (const work of graph.works) {
    const id = workVertexId(work.citation_key);
    vertices.set(id, {
      id,
      kind: "work",
      label: work.title ?? work.citation_key,
      tags: [],
      degree: 0,
      updatedAtMs: null,
      citationKey: work.citation_key,
    });
  }

  const edges: GraphEdge[] = [];
  const connect = (edge: GraphEdge) => {
    const source = vertices.get(edge.source);
    const target = vertices.get(edge.target);
    if (!source || !target) return;
    source.degree += 1;
    target.degree += 1;
    edges.push(edge);
  };
  for (const reference of graph.references) {
    connect({
      id: `reference:${reference.source_note_id}:${reference.target_note_id}`,
      kind: "reference",
      source: reference.source_note_id,
      target: reference.target_note_id,
    });
  }
  for (const citation of graph.citations) {
    connect({
      id: `citation:${citation.source_note_id}:${citation.citation_key}`,
      kind: "citation",
      source: citation.source_note_id,
      target: workVertexId(citation.citation_key),
    });
  }

  return {
    vertices: [...vertices.values()].sort(
      (left, right) =>
        right.degree - left.degree || left.label.localeCompare(right.label),
    ),
    edges,
  };
}
