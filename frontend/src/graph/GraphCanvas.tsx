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
import { formatDateTime } from "../formatting";
import { vertexHref } from "./navigation";
import { GraphEdge, GraphModel, GraphVertex } from "./model";

interface PlacedVertex extends SimulationNodeDatum, GraphVertex {}

/** 選んでいる点と、その画面上の位置。説明の吹き出しをここへ出す。 */
interface VertexDetail {
  vertex: GraphVertex;
  /** 図の左端からの、点の中心の位置。 */
  x: number;
  /** 図の上端からの、点の上端の位置。 */
  y: number;
  /** 図の幅。吹き出しが外へはみ出さないよう内側へ寄せるために使う。 */
  width: number;
}

/** 吹き出しの想定幅。実際の幅は内容で決まるが、寄せ幅の計算にはこの値を使う。 */
const DETAIL_WIDTH = 240;

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
  const [detail, setDetail] = useState<VertexDetail | null>(null);
  const [view, setView] = useState<ZoomTransform>(zoomIdentity);
  const rendered = useRef<GraphModel | null>(null);
  const figure = useRef<HTMLElement>(null);
  const focused = detail?.vertex.id ?? null;

  // 点の画面上の位置を、図の枠を基準にした座標へ直す。拡大や移動をしていても、実際に描かれた
  // 位置から測るため計算がずれない。
  const showDetail = useCallback((vertex: GraphVertex, element: Element) => {
    const frame = figure.current?.getBoundingClientRect();
    if (frame === undefined) return;
    const point = element.getBoundingClientRect();
    setDetail({
      vertex,
      x: point.left + point.width / 2 - frame.left,
      y: point.top - frame.top,
      width: frame.width,
    });
  }, []);

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
    <figure className="graph-canvas" ref={figure}>
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
                aria-label={vertexDescription(vertex)}
                onMouseEnter={(event) =>
                  showDetail(vertex, event.currentTarget)
                }
                onMouseLeave={() => setDetail(null)}
                onFocus={(event) => showDetail(vertex, event.currentTarget)}
                onBlur={() => setDetail(null)}
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
      {detail !== null && <VertexDetailPanel detail={detail} />}
      <figcaption>
        点はノートと文献、線は参照と引用です。点に触れると詳しい情報が出ます。
        点を選ぶとその画面へ移動します。図はドラッグで動かし、ホイールで拡大できます。
        同じ内容は下の一覧からも辿れます。
      </figcaption>
    </figure>
  );
}

/**
 * 選んでいる点の詳しい情報を、その点の横へ出す。
 *
 * 図の枠からはみ出すと読めないため、左右は枠の内側へ寄せる。点より上に出すのは、点の下には
 * 名前が描かれていて重なるためである。
 */
function VertexDetailPanel({ detail }: { detail: VertexDetail }) {
  const { vertex } = detail;
  const left = Math.min(
    Math.max(detail.x + MAXIMUM_RADIUS, DETAIL_WIDTH / 2),
    Math.max(detail.width - DETAIL_WIDTH / 2, DETAIL_WIDTH / 2),
  );
  return (
    <div
      className="graph-detail"
      data-kind={vertex.kind}
      style={{ left: `${left}px`, top: `${detail.y}px` }}
      // 内容は点のaria-labelでも読み上げるため、支援技術へ二重に伝えない。
      aria-hidden="true"
    >
      <p className="graph-detail-label">{vertex.label}</p>
      <dl>
        <div>
          <dt>種類</dt>
          <dd>{vertex.kind === "note" ? "ノート" : "文献"}</dd>
        </div>
        {vertex.updatedAtMs !== null && (
          <div>
            <dt>更新</dt>
            <dd>
              <time dateTime={new Date(vertex.updatedAtMs).toISOString()}>
                {formatDateTime(vertex.updatedAtMs)}
              </time>
            </dd>
          </div>
        )}
        {vertex.citationKey !== null && (
          <div>
            <dt>citation key</dt>
            <dd>
              <code>{vertex.citationKey}</code>
            </dd>
          </div>
        )}
        <div>
          <dt>タグ</dt>
          <dd>{vertex.tags.length === 0 ? "なし" : vertex.tags.join(" / ")}</dd>
        </div>
      </dl>
    </div>
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
