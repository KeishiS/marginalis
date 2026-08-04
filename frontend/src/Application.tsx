import { lazy, Suspense } from "react";

import { ApplicationConfig } from "./api";
import { EditorApplication } from "./EditorApplication";
import { AccessPage } from "./routes/AccessPage";
import { BibliographyPage } from "./routes/BibliographyPage";
import { NoteListPage } from "./routes/NoteListPage";
import { NoteViewPage } from "./routes/NoteViewPage";

// 関係の図は描画に専用の計算を伴う。開かない利用者へ読み込ませないため、経路に入って
// はじめて取得する。
const GraphPage = lazy(() =>
  import("./routes/GraphPage").then((module) => ({
    default: module.GraphPage,
  })),
);
const MathMacroSettingsPage = lazy(() =>
  import("./routes/MathMacroSettingsPage").then((module) => ({
    default: module.MathMacroSettingsPage,
  })),
);

// 起動設定の形は`marginalis-contract`が定めます。ここでは再定義せず再公開します。
export type { ApplicationConfig };

type Route =
  | { kind: "list" }
  | { kind: "create" }
  | { kind: "view"; noteId: string }
  | { kind: "edit"; noteId: string }
  | { kind: "access"; noteId: string }
  | { kind: "bibliography" }
  | { kind: "graph" }
  | { kind: "math-macros" }
  | { kind: "not-found" };

export function Application({ config }: { config: ApplicationConfig }) {
  const route = parseRoute(config.path);
  switch (route.kind) {
    case "list":
      return <NoteListPage config={config} />;
    case "create":
      return (
        <EditorApplication config={{ ...config, mode: "create", noteId: "" }} />
      );
    case "view":
      return <NoteViewPage config={config} noteId={route.noteId} />;
    case "edit":
      return (
        <EditorApplication
          config={{ ...config, mode: "edit", noteId: route.noteId }}
        />
      );
    case "access":
      return <AccessPage config={config} noteId={route.noteId} />;
    case "bibliography":
      return <BibliographyPage config={config} />;
    case "graph":
      return (
        <Suspense
          fallback={
            <p className="state-message" role="status">
              関係の図を読み込んでいます。
            </p>
          }
        >
          <GraphPage config={config} />
        </Suspense>
      );
    case "math-macros":
      return (
        <Suspense
          fallback={
            <p className="state-message" role="status">
              数式マクロ設定を読み込んでいます。
            </p>
          }
        >
          <MathMacroSettingsPage config={config} />
        </Suspense>
      );
    case "not-found":
      return (
        <p className="problem-inline" role="alert">
          指定された画面はありません。
        </p>
      );
  }
}

function parseRoute(pathname: string): Route {
  if (pathname === "/") return { kind: "list" };
  if (pathname === "/bibliography") return { kind: "bibliography" };
  if (pathname === "/graph") return { kind: "graph" };
  if (pathname === "/settings/math-macros") return { kind: "math-macros" };
  if (pathname === "/notes/new") return { kind: "create" };
  const match = pathname.match(/^\/notes\/([^/]+)(?:\/(edit|access))?$/);
  if (!match) return { kind: "not-found" };
  const noteId = decodeURIComponent(match[1]);
  if (match[2] === "edit") return { kind: "edit", noteId };
  if (match[2] === "access") return { kind: "access", noteId };
  return { kind: "view", noteId };
}
