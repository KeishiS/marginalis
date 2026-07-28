import { Problem } from "./api";

export interface EditorActivityState {
  saving: boolean;
  problem: Problem | null;
  notice: string;
}

export type EditorActivityAction =
  | { type: "save-started" }
  | { type: "save-succeeded" }
  | { type: "save-failed"; problem: Problem }
  | { type: "notice"; message: string }
  | { type: "clear-feedback" };

export const initialEditorActivityState: EditorActivityState = {
  saving: false,
  problem: null,
  notice: "",
};

export function editorActivityReducer(
  state: EditorActivityState,
  action: EditorActivityAction,
): EditorActivityState {
  switch (action.type) {
    case "save-started":
      return { saving: true, problem: null, notice: "" };
    case "save-succeeded":
      return { saving: false, problem: null, notice: "保存しました。" };
    case "save-failed":
      return { saving: false, problem: action.problem, notice: "" };
    case "notice":
      return { saving: false, problem: null, notice: action.message };
    case "clear-feedback":
      return state.problem === null && state.notice === ""
        ? state
        : { ...state, problem: null, notice: "" };
  }
}
