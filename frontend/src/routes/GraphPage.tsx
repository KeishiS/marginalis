import { useCallback, useMemo, useState } from "react";

import { ApplicationConfig, NoteGraph, readNoteGraph } from "../api";
import { useApiResource } from "../useApiResource";
import { GraphCanvas } from "../graph/GraphCanvas";
import { GraphList } from "../graph/GraphList";
import { graphModel } from "../graph/model";

/**
 * ノート間の参照と、ノートから文献への引用を図として表示する。
 *
 * 図は情報へ至る唯一の手段にしない。同じ内容を一覧としても示し、キーボードだけで辿れる
 * ようにする。
 */
export function GraphPage({ config }: { config: ApplicationConfig }) {
  const [input, setInput] = useState("");
  const [query, setQuery] = useState("");
  const load = useCallback(
    (signal: AbortSignal) => readNoteGraph(config.apiBase, query, signal),
    [config.apiBase, query],
  );
  const resource = useApiResource<NoteGraph>(load);
  const model = useMemo(
    () => (resource.status === "ready" ? graphModel(resource.value) : null),
    [resource],
  );

  return (
    <section className="page-section graph-page">
      <div className="page-heading">
        <div>
          <p className="page-eyebrow">Graph</p>
          <h1>関係の図</h1>
          <p className="page-description">
            閲覧できるノートと、それらが引用している文献のつながりを示します。
          </p>
        </div>
      </div>

      <form
        className="graph-search"
        onSubmit={(event) => {
          event.preventDefault();
          setQuery(input);
        }}
      >
        <label>
          語で絞り込む
          <input
            value={input}
            onChange={(event) => setInput(event.target.value)}
            placeholder="題名、本文、タグに含まれる語"
          />
        </label>
        <button className="button button-primary" type="submit">
          絞り込む
        </button>
        {query !== "" && (
          <button
            className="button button-secondary"
            type="button"
            onClick={() => {
              setInput("");
              setQuery("");
            }}
          >
            条件を解除
          </button>
        )}
      </form>

      {resource.status === "loading" && (
        <p className="state-message" role="status">
          関係を読み込んでいます。
        </p>
      )}
      {resource.status === "failed" && (
        <p className="problem-inline" role="alert">
          関係を読み込めませんでした。
        </p>
      )}
      {model !== null &&
        (model.vertices.length === 0 ? (
          <p className="state-message">
            {query === ""
              ? "閲覧できるノートはありません。"
              : "条件に一致するノートはありません。"}
          </p>
        ) : (
          <>
            <GraphCanvas config={config} model={model} />
            <GraphList config={config} model={model} />
          </>
        ))}
    </section>
  );
}
