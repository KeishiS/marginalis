import { ApplicationConfig } from "../api";
import { vertexHref } from "./navigation";
import { GraphModel } from "./model";

/**
 * 図と同じ内容を一覧として示す。
 *
 * 図は情報へ至る唯一の手段にしない。キーボードと支援技術だけでも、つながりの多い順に
 * 辿れるようにする。
 */
export function GraphList({
  config,
  model,
}: {
  config: ApplicationConfig;
  model: GraphModel;
}) {
  const linked = (id: string) =>
    model.edges
      .filter((edge) => edge.source === id || edge.target === id)
      .map((edge) => (edge.source === id ? edge.target : edge.source));

  return (
    <details className="graph-outline">
      <summary>つながりの一覧</summary>
      <ul>
        {model.vertices.map((vertex) => {
          const others = linked(vertex.id);
          return (
            <li key={vertex.id}>
              <a href={vertexHref(config, vertex)}>{vertex.label}</a>
              <span className="graph-outline-kind">
                {vertex.kind === "note" ? "ノート" : "文献"}
              </span>
              <span className="graph-outline-degree">
                {others.length === 0
                  ? "つながりなし"
                  : `つながり${others.length}件`}
              </span>
            </li>
          );
        })}
      </ul>
    </details>
  );
}
