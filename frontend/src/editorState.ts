import { Note } from "./api";

export interface EditorForm {
  source: string;
}

export interface EditorConflict {
  editingStarted: EditorForm;
  current: Note;
}

export interface EditorState {
  noteId: string;
  revision: number | null;
  form: EditorForm;
  baseline: EditorForm;
  conflict: EditorConflict | null;
}

export type EditorAction =
  | { type: "change"; field: keyof EditorForm; value: string }
  | { type: "accept-note"; note: Note }
  | { type: "conflict"; current: Note }
  | { type: "clear-conflict" }
  | { type: "rebase"; note: Note };

const EMPTY_FORM: EditorForm = {
  source: "= 新規ノート\n:marginalis-tags:\n:sectnums:\n\n== 見出し1\n\n",
};

export function initialEditorState(noteId: string): EditorState {
  return {
    noteId,
    revision: null,
    form: EMPTY_FORM,
    baseline: EMPTY_FORM,
    conflict: null,
  };
}

export function editorReducer(
  state: EditorState,
  action: EditorAction,
): EditorState {
  switch (action.type) {
    case "change":
      return {
        ...state,
        form: { ...state.form, [action.field]: action.value },
      };
    case "accept-note": {
      const form = noteToForm(action.note);
      return {
        noteId: action.note.note_id,
        revision: action.note.revision,
        form,
        baseline: form,
        conflict: null,
      };
    }
    case "conflict":
      return {
        ...state,
        conflict: { editingStarted: state.baseline, current: action.current },
      };
    case "clear-conflict":
      return { ...state, conflict: null };
    case "rebase":
      return {
        ...state,
        revision: action.note.revision,
        baseline: noteToForm(action.note),
        conflict: null,
      };
  }
}

export function noteToForm(note: Note): EditorForm {
  return {
    source: note.source,
  };
}
