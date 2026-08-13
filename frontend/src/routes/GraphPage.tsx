import { useCallback, useMemo, useState } from "react";

import { ProblemAlert, StatusMessage } from "@/components/feedback";
import { PageHeader } from "@/components/PageHeader";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";

import { ApplicationConfig, NoteGraph, readNoteGraph } from "../api";
import { useApiResource } from "../useApiResource";
import { GraphCanvas } from "../graph/GraphCanvas";
import { GraphList } from "../graph/GraphList";
import { graphModel } from "../graph/model";

/** 起点から辿れる線の本数。公開契約の上限に合わせる。 */
const DEPTHS = [1, 2, 3, 4, 5];

/** URLの検索語、起点、階層を初期の表示範囲として読む。 */
function initialScope(search: string): {
  query: string;
  origin: string;
  depth: number;
} {
  const parameters = new URLSearchParams(search);
  const depth = Number(parameters.get("depth"));
  return {
    query: parameters.get("query") ?? "",
    origin: parameters.get("origin") ?? "",
    depth: DEPTHS.includes(depth) ? depth : 1,
  };
}

function replaceGraphSearch({
  query,
  origin,
  depth,
}: {
  query: string;
  origin: string;
  depth: number;
}) {
  const parameters = new URLSearchParams();
  if (query) parameters.set("query", query);
  if (origin) {
    parameters.set("origin", origin);
    parameters.set("depth", String(depth));
  }
  const search = parameters.toString();
  window.history.replaceState(
    window.history.state,
    "",
    `${window.location.pathname}${search ? `?${search}` : ""}${window.location.hash}`,
  );
}

/**
 * ノート間の参照と、ノートから文献への引用を図として表示する。
 *
 * 図は情報へ至る唯一の手段にしない。同じ内容を一覧としても示し、キーボードだけで辿れる
 * ようにする。
 */
export function GraphPage({ config }: { config: ApplicationConfig }) {
  const [scope, setScope] = useState(() => initialScope(config.search));
  const [input, setInput] = useState(scope.query);
  const { query, origin, depth } = scope;
  const changeScope = useCallback((next: typeof scope) => {
    setScope(next);
    replaceGraphSearch(next);
  }, []);
  const load = useCallback(
    (signal: AbortSignal) =>
      readNoteGraph(config.apiBase, { query, origin, depth }, signal),
    [config.apiBase, query, origin, depth],
  );
  const resource = useApiResource<NoteGraph>(load);
  const model = useMemo(
    () => (resource.status === "ready" ? graphModel(resource.value) : null),
    [resource],
  );
  const originTitle =
    resource.status === "ready"
      ? resource.value.notes.find((note) => note.note_id === origin)?.title
      : undefined;

  return (
    <section className="grid gap-6">
      <PageHeader
        eyebrow="Graph"
        title="関係の図"
        description="閲覧できるノートと、それらが引用している文献のつながりを示します。"
      />

      <form
        className="flex flex-wrap items-end gap-3"
        onSubmit={(event) => {
          event.preventDefault();
          changeScope({ ...scope, query: input });
        }}
      >
        <label className="grid min-w-[min(20rem,78vw)] gap-1 text-sm font-semibold">
          語で絞り込む
          <Input
            value={input}
            onChange={(event) => setInput(event.target.value)}
            placeholder="題名、本文、タグに含まれる語"
          />
        </label>
        <Button variant="outline" type="submit">
          絞り込む
        </Button>
        {query !== "" && (
          <Button
            variant="outline"
            type="button"
            onClick={() => {
              setInput("");
              changeScope({ ...scope, query: "" });
            }}
          >
            条件を解除
          </Button>
        )}
      </form>

      {origin !== "" && (
        <div className="graph-origin">
          <p>
            <strong>{originTitle ?? "選んだノート"}</strong>
            を起点に表示しています。
          </p>
          <label>
            辿る階層
            <select
              value={depth}
              onChange={(event) =>
                changeScope({
                  ...scope,
                  depth: Number(event.target.value),
                })
              }
            >
              {DEPTHS.map((value) => (
                <option key={value} value={value}>
                  {value}
                </option>
              ))}
            </select>
          </label>
          <Button
            variant="outline"
            type="button"
            onClick={() => changeScope({ ...scope, origin: "", depth: 1 })}
          >
            全体を見る
          </Button>
        </div>
      )}

      {resource.status === "loading" && (
        <StatusMessage>関係を読み込んでいます。</StatusMessage>
      )}
      {resource.status === "failed" && (
        <ProblemAlert>関係を読み込めませんでした。</ProblemAlert>
      )}
      {model !== null &&
        (model.vertices.length === 0 ? (
          <StatusMessage>
            {origin !== ""
              ? "起点にしたノートが見つかりません。"
              : query === ""
                ? "閲覧できるノートはありません。"
                : "条件に一致するノートはありません。"}
          </StatusMessage>
        ) : (
          <>
            <GraphCanvas config={config} model={model} />
            <GraphList config={config} model={model} />
          </>
        ))}
    </section>
  );
}
