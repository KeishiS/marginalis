import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from "react";
import type { ZoomTransform } from "d3-zoom";

import type { GraphModel, GraphVertex } from "./model";
import type { VertexDetail } from "./VertexDetail";

/** 点から吹き出しへマウスを移す間、表示を保つ時間。 */
const DETAIL_HIDE_DELAY_MS = 120;

interface VertexTarget {
  model: GraphModel;
  vertex: GraphVertex;
  element: Element;
}

/**
 * 点のホバーとフォーカスから吹き出しの表示状態を管理する。
 *
 * 点からスクロール可能な吹き出しへマウスを移せるよう、マウスが点を離れた場合だけ短い猶予を
 * 設ける。キーボードのフォーカスが残っている間は表示を保つ。
 */
export function useVertexDetail({
  stage,
  view,
  model,
}: {
  stage: HTMLDivElement | null;
  view: ZoomTransform;
  model: GraphModel;
}) {
  const [storedDetail, setStoredDetail] = useState<{
    model: GraphModel;
    detail: VertexDetail;
  } | null>(null);
  const active = useRef<VertexTarget | null>(null);
  const focusedTarget = useRef<VertexTarget | null>(null);
  const vertexHovered = useRef(false);
  const vertexFocused = useRef(false);
  const detailHovered = useRef(false);
  const hideTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  // Reactが新しい図を描く前に、以前の図の点や内容を表示対象から外す。条件付きの更新なので、
  // 古い吹き出しを一度描画してからEffectで消す中間状態を作らない。
  if (storedDetail !== null && storedDetail.model !== model) {
    setStoredDetail(null);
  }
  const detail = storedDetail?.model === model ? storedDetail.detail : null;

  // 点の画面上の位置を、図本体を基準にした座標へ直す。SVGの変換式を複製せず、実際の描画位置を
  // 測るため、拡大や移動をしていても計算がずれない。
  const update = useCallback(() => {
    const target = active.current;
    if (target === null || target.model !== model || stage === null) return;
    const frame = stage.getBoundingClientRect();
    const point = target.element.getBoundingClientRect();
    setStoredDetail({
      model,
      detail: {
        vertex: target.vertex,
        x: point.left + point.width / 2 - frame.left,
        top: point.top - frame.top,
        bottom: point.bottom - frame.top,
        width: frame.width,
        height: frame.height,
      },
    });
  }, [model, stage]);

  const show = useCallback(
    (vertex: GraphVertex, element: Element) => {
      active.current = { model, vertex, element };
      update();
    },
    [model, update],
  );

  const cancelScheduledHide = useCallback(() => {
    if (hideTimer.current === null) return;
    clearTimeout(hideTimer.current);
    hideTimer.current = null;
  }, []);

  const hide = useCallback(() => {
    cancelScheduledHide();
    active.current = null;
    setStoredDetail(null);
  }, [cancelScheduledHide]);

  const restoreFocusedOrHide = useCallback(() => {
    const target = focusedTarget.current;
    if (vertexFocused.current && target !== null && target.model === model) {
      active.current = target;
      update();
      return;
    }
    hide();
  }, [hide, model, update]);

  const hideIfInactive = useCallback(() => {
    if (
      !vertexHovered.current &&
      !vertexFocused.current &&
      !detailHovered.current
    ) {
      hide();
    } else if (!vertexHovered.current && !detailHovered.current) {
      restoreFocusedOrHide();
    }
  }, [hide, restoreFocusedOrHide]);

  const scheduleHide = useCallback(() => {
    cancelScheduledHide();
    hideTimer.current = setTimeout(hideIfInactive, DETAIL_HIDE_DELAY_MS);
  }, [cancelScheduledHide, hideIfInactive]);

  useEffect(() => () => cancelScheduledHide(), [cancelScheduledHide]);

  useEffect(() => {
    // DOM要素と図全体への参照も、新しい図へ切り替わった時点で解放する。
    cancelScheduledHide();
    active.current = null;
    focusedTarget.current = null;
    vertexHovered.current = false;
    vertexFocused.current = false;
    detailHovered.current = false;
  }, [cancelScheduledHide, model]);

  useLayoutEffect(() => {
    // viewの変更がDOMへ反映された後に測る。点へフォーカスしたまま拡大や移動をしても、新しい
    // 点の位置へ追従する。
    update();
  }, [update, view]);

  useEffect(() => {
    if (stage === null || typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(update);
    observer.observe(stage);
    return () => observer.disconnect();
  }, [stage, update]);

  return {
    detail,
    focusedVertexId: detail?.vertex.id ?? null,
    showFromMouse(vertex: GraphVertex, element: Element) {
      vertexHovered.current = true;
      cancelScheduledHide();
      show(vertex, element);
    },
    leaveVertex() {
      vertexHovered.current = false;
      scheduleHide();
    },
    showFromFocus(vertex: GraphVertex, element: Element) {
      vertexFocused.current = true;
      focusedTarget.current = { model, vertex, element };
      cancelScheduledHide();
      show(vertex, element);
    },
    blurVertex() {
      vertexFocused.current = false;
      focusedTarget.current = null;
      hideIfInactive();
    },
    enterPanel() {
      detailHovered.current = true;
      cancelScheduledHide();
    },
    leavePanel() {
      detailHovered.current = false;
      scheduleHide();
    },
  };
}
