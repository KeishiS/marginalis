import { ApplicationConfig } from "./api";
import { EditorApplication } from "./EditorApplication";
import { AccessPage } from "./routes/AccessPage";
import { BibliographyPage } from "./routes/BibliographyPage";
import { NoteListPage } from "./routes/NoteListPage";
import { NoteViewPage } from "./routes/NoteViewPage";

// 起動設定の形は`marginalis-contract`が定めます。ここでは再定義せず再公開します。
export type { ApplicationConfig };

type Route =
  | { kind: "list" }
  | { kind: "create" }
  | { kind: "view"; noteId: string }
  | { kind: "edit"; noteId: string }
  | { kind: "access"; noteId: string }
  | { kind: "bibliography" }
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
  if (pathname === "/notes/new") return { kind: "create" };
  const match = pathname.match(/^\/notes\/([^/]+)(?:\/(edit|access))?$/);
  if (!match) return { kind: "not-found" };
  const noteId = decodeURIComponent(match[1]);
  if (match[2] === "edit") return { kind: "edit", noteId };
  if (match[2] === "access") return { kind: "access", noteId };
  return { kind: "view", noteId };
}
