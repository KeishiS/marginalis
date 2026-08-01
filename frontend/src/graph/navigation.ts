import { ApplicationConfig } from "../api";
import { externalPath, notePath } from "../paths";
import { GraphVertex } from "./model";

/**
 * 点を選んだときの移動先。
 *
 * ノートは閲覧画面、文献は書誌ライブラリーのその項目を絞り込んだ状態を指す。文献ごとの
 * 画面は無いため、citation keyで絞った一覧を移動先とする。
 */
export function vertexHref(
  config: ApplicationConfig,
  vertex: GraphVertex,
): string {
  if (vertex.kind === "note") {
    return notePath({ basePath: config.basePath, search: "" }, vertex.id);
  }
  const citationKey = vertex.id.slice("work:".length);
  return `${externalPath(config.basePath, "/bibliography")}?query=${encodeURIComponent(citationKey)}`;
}
