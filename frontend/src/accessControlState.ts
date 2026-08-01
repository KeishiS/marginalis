import { NoteAclEntry, NotePermission } from "./api";

export interface AccessControlState {
  status: "loading" | "ready" | "saving";
  entries: NoteAclEntry[] | null;
  subject: string;
  permission: NotePermission;
  revision: number;
  notice: string;
  error: string;
}

export type AccessControlAction =
  | { type: "loading" }
  | { type: "loaded"; entries: NoteAclEntry[]; revision: number }
  | { type: "subject"; value: string }
  | { type: "permission"; value: NotePermission }
  | { type: "add" }
  | { type: "remove"; subject: string }
  | { type: "save-started" }
  | { type: "saved"; revision: number }
  | { type: "error"; message: string };

export function initialAccessControlState(
  revision: number,
): AccessControlState {
  return {
    status: "loading",
    entries: null,
    subject: "",
    permission: "read",
    revision,
    notice: "",
    error: "",
  };
}

export function accessControlReducer(
  state: AccessControlState,
  action: AccessControlAction,
): AccessControlState {
  switch (action.type) {
    case "loading":
      return {
        ...state,
        status: "loading",
        entries: null,
        notice: "",
        error: "",
      };
    case "loaded":
      return {
        ...state,
        status: "ready",
        entries: action.entries,
        revision: action.revision,
        error: "",
      };
    case "subject":
      return { ...state, subject: action.value };
    case "permission":
      return { ...state, permission: action.value };
    case "add": {
      const subject = state.subject.trim();
      if (!subject || state.entries === null) {
        return {
          ...state,
          error: "共有する利用者のsubjectを入力してください。",
        };
      }
      return {
        ...state,
        entries: [
          ...state.entries.filter((entry) => entry.subject !== subject),
          { subject, permission: state.permission },
        ],
        subject: "",
        error: "",
        notice: "未保存の共有設定があります。",
      };
    }
    case "remove":
      return {
        ...state,
        entries:
          state.entries?.filter((entry) => entry.subject !== action.subject) ??
          null,
        notice: "未保存の共有設定があります。",
      };
    case "save-started":
      return { ...state, status: "saving", notice: "", error: "" };
    case "saved":
      return {
        ...state,
        status: "ready",
        revision: action.revision,
        notice: "共有設定を保存しました。",
        error: "",
      };
    case "error":
      return { ...state, status: "ready", error: action.message };
  }
}
