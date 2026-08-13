// smoke spec間で共有する固定値とスクリーンショット設定。

const pendingWebProvenance = {
  created_via: "web",
  review_status: "pending",
  reviewed_revision: null,
  reviewed_at_ms: null,
};

function escapeHtml(value) {
  return value
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;");
}

const SCREENSHOT_OPTIONS = {
  animations: "disabled",
  // Linux環境ごとのフォント描画差を許容し、配置の大きな崩れは検出します。
  // 文字量の多い画面では描画環境の差だけで3%を超えることがあるため、5%とします。
  maxDiffPixelRatio: 0.05,
};

/**
 * 編集欄の中身を隠して比較する。
 *
 * CodeMirrorが描く文字は環境ごとの差が大きく、実行環境を変えると配置が同じでも許容差を
 * 超える。隠す対象は入力した文字だけで、行番号、枠、操作、分割の位置は比較に残る。
 */
function editorScreenshotOptions(page) {
  return { ...SCREENSHOT_OPTIONS, mask: [page.locator(".cm-content")] };
}

/**
 * 日時の表示を隠して比較する。
 *
 * 日時は実行環境の地域と時間帯で文字列が変わる。値そのものはDOMの`datetime`属性で確かめ、
 * 画像では吹き出しの位置と大きさだけを比較する。
 */
function detailScreenshotOptions(page) {
  return { ...SCREENSHOT_OPTIONS, mask: [page.locator(".graph-detail time")] };
}

module.exports = {
  pendingWebProvenance,
  escapeHtml,
  SCREENSHOT_OPTIONS,
  editorScreenshotOptions,
  detailScreenshotOptions,
};
