import { useLayoutEffect, useRef, useState } from "react";

import { formatDateTime } from "../formatting";
import type { GraphVertex } from "./model";

/** 選んでいる点と、図本体を基準にした画面上の位置。 */
export interface VertexDetail {
  vertex: GraphVertex;
  /** 点の中心の横位置。 */
  x: number;
  /** 点の上端と下端の縦位置。 */
  top: number;
  bottom: number;
  /** 図本体の寸法。 */
  width: number;
  height: number;
}

interface DetailPosition {
  left: number;
  top: number;
}

/** 吹き出しと図の端または点との間に空ける幅。`--space-2`と同じ8px。 */
const DETAIL_GAP = 8;

/**
 * 選んでいる点の詳しい情報を、その点の近くへ出す。
 *
 * 図の枠からはみ出すと読めないため、実寸に合わせて上下左右を枠の内側へ寄せる。点より上を
 * 優先するのは、点の下には名前が描かれていて重なるためである。
 */
export function VertexDetailPanel({
  detail,
  onMouseEnter,
  onMouseLeave,
}: {
  detail: VertexDetail;
  onMouseEnter: () => void;
  onMouseLeave: () => void;
}) {
  const { vertex } = detail;
  const panel = useRef<HTMLDivElement>(null);
  const [position, setPosition] = useState<DetailPosition | null>(null);

  useLayoutEffect(() => {
    const element = panel.current;
    if (element === null) return;
    const updatePosition = () => {
      const bounds = element.getBoundingClientRect();
      setPosition(placeDetail(detail, bounds.width, bounds.height));
    };
    updatePosition();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(updatePosition);
    observer.observe(element);
    return () => observer.disconnect();
  }, [detail]);

  return (
    <div
      ref={panel}
      className="graph-detail"
      data-kind={vertex.kind}
      onMouseEnter={onMouseEnter}
      onMouseLeave={onMouseLeave}
      style={
        position === null
          ? { visibility: "hidden" }
          : { left: `${position.left}px`, top: `${position.top}px` }
      }
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

/** 実寸を使い、点の上を優先しながら吹き出し全体を図の内側へ置く。 */
function placeDetail(
  detail: VertexDetail,
  panelWidth: number,
  panelHeight: number,
): DetailPosition {
  const maximumLeft = Math.max(
    DETAIL_GAP,
    detail.width - panelWidth - DETAIL_GAP,
  );
  const left = clamp(detail.x - panelWidth / 2, DETAIL_GAP, maximumLeft);

  const above = detail.top - DETAIL_GAP - panelHeight;
  const below = detail.bottom + DETAIL_GAP;
  const maximumTop = Math.max(
    DETAIL_GAP,
    detail.height - panelHeight - DETAIL_GAP,
  );
  const preferredTop =
    above >= DETAIL_GAP || below > maximumTop ? above : below;
  return {
    left,
    top: clamp(preferredTop, DETAIL_GAP, maximumTop),
  };
}

function clamp(value: number, minimum: number, maximum: number): number {
  return Math.min(Math.max(value, minimum), maximum);
}
