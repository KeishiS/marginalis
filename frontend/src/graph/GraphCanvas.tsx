import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { select } from "d3-selection";
import { zoom, zoomIdentity, type ZoomTransform } from "d3-zoom";
import {
  forceCenter,
  forceLink,
  forceManyBody,
  forceSimulation,
  forceX,
  forceY,
  type SimulationLinkDatum,
  type SimulationNodeDatum,
} from "d3-force";

import { ApplicationConfig } from "../api";
import { vertexHref } from "./navigation";
import { GraphEdge, GraphModel, GraphVertex } from "./model";

interface PlacedVertex extends SimulationNodeDatum, GraphVertex {}

const VIEW_WIDTH = 1200;
const VIEW_HEIGHT = 720;
/** 点の半径。つながりが多いほど大きくするが、上限を設けて図を埋めない。 */
const MINIMUM_RADIUS = 10;
const MAXIMUM_RADIUS = 26;

/**
 * 力の釣り合いで点を配置し、静止した図として描く。
 *
 * 配置は取得のたびに一度だけ計算し、その後は動かさない。動き続ける図は目で追いにくく、
 * 動きを減らす設定とも相容れない。
 */
export function GraphCanvas({
  config,
  model,
}: {
  config: ApplicationConfig;
  model: GraphModel;
}) {
  const [placed, setPlaced] = useState<PlacedVertex[] | null>(null);
  const [focused, setFocused] = useState<string | null>(null);
  const [view, setView] = useState<ZoomTransform>(zoomIdentity);
  const rendered = useRef<GraphModel | null>(null);

  // 1,000点を一画面へ収めると読めない。拡大と移動で見たい範囲へ寄れるようにする。
  const attachZoom = useCallback((element: SVGSVGElement | null) => {
    if (element === null) return;
    const behaviour = zoom<SVGSVGElement, unknown>()
      .scaleExtent([0.2, 4])
      .on("zoom", (event) => setView(event.transform));
    select(element).call(behaviour);
  }, []);

  useEffect(() => {
    if (rendered.current === model) return;
    rendered.current = model;
    const vertices: PlacedVertex[] = model.vertices.map((vertex) => ({
      ...vertex,
    }));
    const links: SimulationLinkDatum<PlacedVertex>[] = model.edges.map(
      (edge) => ({ source: edge.source, target: edge.target }),
    );
    const simulation = forceSimulation(vertices)
      .force(
        "link",
        forceLink<PlacedVertex, SimulationLinkDatum<PlacedVertex>>(links)
          .id((vertex) => vertex.id)
          .distance(110)
          .strength(0.6),
      )
      .force("charge", forceManyBody().strength(-320).distanceMax(520))
      .force("center", forceCenter(VIEW_WIDTH / 2, VIEW_HEIGHT / 2))
      .force("x", forceX(VIEW_WIDTH / 2).strength(0.03))
      .force("y", forceY(VIEW_HEIGHT / 2).strength(0.05))
      .stop();
    // 画面を再描画せずに収束させる。1,000点でも一度きりの計算で済ませる。
    simulation.tick(300);
    setPlaced(vertices);
    return () => {
      simulation.stop();
    };
  }, [model]);

  const positions = useMemo(() => {
    const map = new Map<string, PlacedVertex>();
    for (const vertex of placed ?? []) map.set(vertex.id, vertex);
    return map;
  }, [placed]);

  if (placed === null) {
    return (
      <p className="state-message" role="status">
        配置を計算しています。
      </p>
    );
  }

  const radius = (vertex: GraphVertex) =>
    Math.min(MINIMUM_RADIUS + vertex.degree * 2, MAXIMUM_RADIUS);
  const neighbours = (id: string) =>
    model.edges.some((edge) => edge.source === id || edge.target === id);

  return (
    <figure className="graph-canvas">
      <svg
        ref={attachZoom}
        viewBox={`0 0 ${VIEW_WIDTH} ${VIEW_HEIGHT}`}
        role="img"
        aria-label="ノートと文献のつながり"
      >
        <g transform={view.toString()}>
          <g className="graph-edges">
            {model.edges.map((edge) => (
              <GraphLine
                key={edge.id}
                edge={edge}
                positions={positions}
                dimmed={focused !== null && !touches(edge, focused)}
              />
            ))}
          </g>
          <g className="graph-vertices">
            {placed.map((vertex) => (
              <a
                key={vertex.id}
                className="graph-vertex"
                href={vertexHref(config, vertex)}
                data-kind={vertex.kind}
                data-isolated={neighbours(vertex.id) ? undefined : "true"}
                aria-label={`${vertex.label}（${
                  vertex.kind === "note" ? "ノート" : "文献"
                }、つながり${vertex.degree}件）`}
                onMouseEnter={() => setFocused(vertex.id)}
                onMouseLeave={() => setFocused(null)}
                onFocus={() => setFocused(vertex.id)}
                onBlur={() => setFocused(null)}
              >
                <circle
                  cx={vertex.x ?? 0}
                  cy={vertex.y ?? 0}
                  r={radius(vertex)}
                />
                <text
                  x={vertex.x ?? 0}
                  y={(vertex.y ?? 0) + radius(vertex) + 14}
                  textAnchor="middle"
                >
                  {vertex.label}
                </text>
              </a>
            ))}
          </g>
        </g>
      </svg>
      <figcaption>
        点はノートと文献、線は参照と引用です。点を選ぶとその画面へ移動します。
        図はドラッグで動かし、ホイールで拡大できます。同じ内容は下の一覧からも辿れます。
      </figcaption>
    </figure>
  );
}

function GraphLine({
  edge,
  positions,
  dimmed,
}: {
  edge: GraphEdge;
  positions: Map<string, PlacedVertex>;
  dimmed: boolean;
}) {
  const source = positions.get(edge.source);
  const target = positions.get(edge.target);
  if (!source || !target) return null;
  return (
    <line
      className="graph-edge"
      data-kind={edge.kind}
      data-dimmed={dimmed ? "true" : undefined}
      x1={source.x ?? 0}
      y1={source.y ?? 0}
      x2={target.x ?? 0}
      y2={target.y ?? 0}
    />
  );
}

function touches(edge: GraphEdge, id: string): boolean {
  return edge.source === id || edge.target === id;
}
