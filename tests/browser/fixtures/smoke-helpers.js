// smoke spec間で共有する固定値。

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

module.exports = {
  pendingWebProvenance,
  escapeHtml,
};
