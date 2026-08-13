import { lazy, Suspense } from "react";

import { ProblemAlert, StatusMessage } from "@/components/feedback";

import { ApplicationConfig } from "./api";
import { AccessPage } from "./routes/AccessPage";
import { BibliographyPage } from "./routes/BibliographyPage";
import { NoteListPage } from "./routes/NoteListPage";
import { NoteViewPage } from "./routes/NoteViewPage";
import { SettingsPage } from "./routes/SettingsPage";

// 関係の図は描画に専用の計算を伴う。開かない利用者へ読み込ませないため、経路に入って
// はじめて取得する。
const GraphPage = lazy(() =>
  import("./routes/GraphPage").then((module) => ({
    default: module.GraphPage,
  })),
);
const EditorApplication = lazy(() =>
  import("./EditorApplication").then((module) => ({
    default: module.EditorApplication,
  })),
);
const MathMacroSettingsPage = lazy(() =>
  import("./routes/MathMacroSettingsPage").then((module) => ({
    default: module.MathMacroSettingsPage,
  })),
);
const McpAccessSettingsPage = lazy(() =>
  import("./routes/McpAccessSettingsPage").then((module) => ({
    default: module.McpAccessSettingsPage,
  })),
);
const WebhookSettingsPage = lazy(() =>
  import("./routes/WebhookSettingsPage").then((module) => ({
    default: module.WebhookSettingsPage,
  })),
);
const DeletedNotesPage = lazy(() =>
  import("./routes/DeletedNotesPage").then((module) => ({
    default: module.DeletedNotesPage,
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
  | { kind: "settings" }
  | { kind: "math-macros" }
  | { kind: "mcp-access-settings" }
  | { kind: "webhook-settings" }
  | { kind: "deleted-notes" }
  | { kind: "not-found" };

export function Application({ config }: { config: ApplicationConfig }) {
  const route = parseRoute(config.path);
  switch (route.kind) {
    case "list":
      return <NoteListPage config={config} />;
    case "create":
      return <EditorRoute config={config} mode="create" noteId="" />;
    case "view":
      return <NoteViewPage config={config} noteId={route.noteId} />;
    case "edit":
      return <EditorRoute config={config} mode="edit" noteId={route.noteId} />;
    case "access":
      return <AccessPage config={config} noteId={route.noteId} />;
    case "bibliography":
      return <BibliographyPage config={config} />;
    case "graph":
      return (
        <Suspense
          fallback={<StatusMessage>関係の図を読み込んでいます。</StatusMessage>}
        >
          <GraphPage config={config} />
        </Suspense>
      );
    case "settings":
      return <SettingsPage config={config} />;
    case "math-macros":
      return (
        <Suspense
          fallback={
            <StatusMessage>数式マクロ設定を読み込んでいます。</StatusMessage>
          }
        >
          <MathMacroSettingsPage config={config} />
        </Suspense>
      );
    case "mcp-access-settings":
      return (
        <Suspense
          fallback={
            <StatusMessage>MCPのアクセス設定を読み込んでいます。</StatusMessage>
          }
        >
          <McpAccessSettingsPage config={config} />
        </Suspense>
      );
    case "webhook-settings":
      return (
        <Suspense
          fallback={
            <StatusMessage>Webhookの設定を読み込んでいます。</StatusMessage>
          }
        >
          <WebhookSettingsPage config={config} />
        </Suspense>
      );
    case "deleted-notes":
      return (
        <Suspense
          fallback={
            <StatusMessage>削除済みノートを読み込んでいます。</StatusMessage>
          }
        >
          <DeletedNotesPage config={config} />
        </Suspense>
      );
    case "not-found":
      return <ProblemAlert>指定された画面はありません。</ProblemAlert>;
  }
}

function EditorRoute({
  config,
  mode,
  noteId,
}: {
  config: ApplicationConfig;
  mode: "create" | "edit";
  noteId: string;
}) {
  return (
    <Suspense
      fallback={<StatusMessage>編集画面を読み込んでいます。</StatusMessage>}
    >
      <EditorApplication config={{ ...config, mode, noteId }} />
    </Suspense>
  );
}

function parseRoute(pathname: string): Route {
  if (pathname === "/") return { kind: "list" };
  if (pathname === "/bibliography") return { kind: "bibliography" };
  if (pathname === "/graph") return { kind: "graph" };
  if (pathname === "/settings") return { kind: "settings" };
  if (pathname === "/settings/math-macros") return { kind: "math-macros" };
  if (pathname === "/settings/mcp-access")
    return { kind: "mcp-access-settings" };
  if (pathname === "/settings/webhooks") return { kind: "webhook-settings" };
  if (pathname === "/notes/deleted") return { kind: "deleted-notes" };
  if (pathname === "/notes/new") return { kind: "create" };
  const match = pathname.match(/^\/notes\/([^/]+)(?:\/(edit|access))?$/);
  if (!match) return { kind: "not-found" };
  const noteId = decodeURIComponent(match[1]);
  if (match[2] === "edit") return { kind: "edit", noteId };
  if (match[2] === "access") return { kind: "access", noteId };
  return { kind: "view", noteId };
}
