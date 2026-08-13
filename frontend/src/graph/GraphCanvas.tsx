import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { StatusMessage } from "@/components/feedback";
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
import { formatDateTime } from "../formatting";
import { vertexHref } from "./navigation";
import { GraphEdge, GraphModel, GraphVertex } from "./model";
import { VertexDetailPanel } from "./VertexDetail";
import { useVertexDetail } from "./useVertexDetail";

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
  const [view, setView] = useState<ZoomTransform>(zoomIdentity);
  const [stage, setStage] = useState<HTMLDivElement | null>(null);
  const rendered = useRef<GraphModel | null>(null);
  const vertexDetail = useVertexDetail({
    stage,
    view,
    model,
  });

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

  // 点の広がりに合わせて表示範囲を決める。固定の範囲にすると、点が少ないときに余白ばかりの
  // 図になり、多いときは端が切れる。
  const bounds = useMemo(() => fitBounds(placed), [placed]);

  if (placed === null) {
    return <StatusMessage>配置を計算しています。</StatusMessage>;
  }

  const radius = (vertex: GraphVertex) =>
    Math.min(MINIMUM_RADIUS + vertex.degree * 2, MAXIMUM_RADIUS);
  const neighbours = (id: string) =>
    model.edges.some((edge) => edge.source === id || edge.target === id);

  return (
    <figure className="graph-canvas">
      <div className="graph-stage" ref={setStage}>
        <svg
          ref={attachZoom}
          viewBox={`${bounds.x} ${bounds.y} ${bounds.width} ${bounds.height}`}
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
                  dimmed={
                    vertexDetail.focusedVertexId !== null &&
                    !touches(edge, vertexDetail.focusedVertexId)
                  }
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
                  aria-label={vertexDescription(vertex)}
                  onMouseEnter={(event) =>
                    vertexDetail.showFromMouse(vertex, event.currentTarget)
                  }
                  onMouseLeave={vertexDetail.leaveVertex}
                  onFocus={(event) =>
                    vertexDetail.showFromFocus(vertex, event.currentTarget)
                  }
                  onBlur={vertexDetail.blurVertex}
                >
                  <circle
                    cx={vertex.x ?? 0}
                    cy={vertex.y ?? 0}
                    r={radius(vertex)}
                  />
                  {vertex.kind === "note" && (
                    <text
                      x={vertex.x ?? 0}
                      y={(vertex.y ?? 0) + radius(vertex) + 14}
                      textAnchor="middle"
                    >
                      {vertex.label}
                    </text>
                  )}
                </a>
              ))}
            </g>
          </g>
        </svg>
        {vertexDetail.detail !== null && (
          <VertexDetailPanel
            detail={vertexDetail.detail}
            onMouseEnter={vertexDetail.enterPanel}
            onMouseLeave={vertexDetail.leavePanel}
          />
        )}
      </div>
      <figcaption>
        点はノートと文献、線は参照と引用です。点に触れると詳しい情報が出ます。
        点を選ぶとその画面へ移動します。図はドラッグで動かし、ホイールで拡大できます。
        同じ内容は下の一覧からも辿れます。
      </figcaption>
    </figure>
  );
}

/**
 * 点の説明。支援技術へは吹き出しではなくこの文言で伝える。
 *
 * 吹き出しはマウスの位置に依存し、読み上げの順序にも乗らない。同じ内容を点自身の名前として
 * 持たせ、キーボードだけでも同じことが分かるようにする。
 */
function vertexDescription(vertex: GraphVertex): string {
  const parts = [vertex.label, vertex.kind === "note" ? "ノート" : "文献"];
  if (vertex.updatedAtMs !== null) {
    parts.push(`更新${formatDateTime(vertex.updatedAtMs)}`);
  }
  if (vertex.citationKey !== null) {
    parts.push(`citation key ${vertex.citationKey}`);
  }
  parts.push(
    vertex.tags.length === 0 ? "タグなし" : `タグ${vertex.tags.join("、")}`,
  );
  parts.push(`つながり${vertex.degree}件`);
  return parts.join("、");
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

/**
 * 点がすべて入る表示範囲を返す。
 *
 * 名前は点の下へ出るため、下側の余白を広く取る。点が1つだけの場合も潰れないよう、最小の
 * 大きさを保つ。
 */
function fitBounds(placed: PlacedVertex[] | null): {
  x: number;
  y: number;
  width: number;
  height: number;
} {
  const points = placed ?? [];
  if (points.length === 0) {
    return { x: 0, y: 0, width: VIEW_WIDTH, height: VIEW_HEIGHT };
  }
  const xs = points.map((vertex) => vertex.x ?? 0);
  const ys = points.map((vertex) => vertex.y ?? 0);
  const margin = MAXIMUM_RADIUS + 32;
  // 点が少ないときに拡大されすぎないよう、最小の範囲を保つ。点の中心から広げるため、
  // 図は常に中央へ寄る。
  const width = Math.max(
    Math.max(...xs) - Math.min(...xs) + margin * 2,
    VIEW_WIDTH / 2,
  );
  const height = Math.max(
    Math.max(...ys) - Math.min(...ys) + margin * 2,
    VIEW_HEIGHT / 2,
  );
  const centerX = (Math.min(...xs) + Math.max(...xs)) / 2;
  const centerY = (Math.min(...ys) + Math.max(...ys)) / 2;
  return {
    x: centerX - width / 2,
    y: centerY - height / 2,
    width,
    height,
  };
}

function touches(edge: GraphEdge, id: string): boolean {
  return edge.source === id || edge.target === id;
}
