import { describe, expect, it } from "vitest";

import {
  accessControlReducer,
  initialAccessControlState,
} from "../src/accessControlState";
import {
  editorActivityReducer,
  initialEditorActivityState,
} from "../src/editorActivityState";
import { editorReducer, initialEditorState } from "../src/editorState";

const note = {
  note_id: "0197c9bc-0000-7000-8000-000000000001",
  title: "保存済み",
  source: "= 保存済み\n:marginalis-tags: 設計\n\n本文",
  tags: ["設計"],
  created_at_ms: 1,
  updated_at_ms: 2,
  revision: 2,
  created_via: "web" as const,
  review_status: "pending" as const,
  reviewed_revision: null,
  reviewed_at_ms: null,
};

describe("editorReducer", () => {
  it("保存応答を編集内容と比較基準へ原子的に反映する", () => {
    const changed = editorReducer(initialEditorState(""), {
      type: "change",
      field: "source",
      value: "= 編集中\n",
    });
    const saved = editorReducer(changed, { type: "accept-note", note });
    expect(saved.form).toEqual(saved.baseline);
    expect(saved.noteId).toBe(note.note_id);
    expect(saved.revision).toBe(2);
  });

  it("競合解消では編集中の内容を保って比較基準だけを更新する", () => {
    const loaded = editorReducer(initialEditorState(note.note_id), {
      type: "accept-note",
      note,
    });
    const editing = editorReducer(loaded, {
      type: "change",
      field: "source",
      value: "= 保存済み\n\n編集中の本文",
    });
    const current = { ...note, source: "= 保存済み\n\n他の更新", revision: 3 };
    const rebased = editorReducer(
      editorReducer(editing, { type: "conflict", current }),
      { type: "rebase", note: current },
    );
    expect(rebased.form.source).toContain("編集中の本文");
    expect(rebased.baseline.source).toContain("他の更新");
    expect(rebased.revision).toBe(3);
  });
});

describe("editorActivityReducer", () => {
  it("保存の開始、失敗、入力再開を一貫した状態へ遷移させる", () => {
    const saving = editorActivityReducer(initialEditorActivityState, {
      type: "save-started",
    });
    expect(saving).toMatchObject({ saving: true, problem: null, notice: "" });
    const failed = editorActivityReducer(saving, {
      type: "save-failed",
      problem: { code: "network_error", message: "通信失敗" },
    });
    expect(failed).toMatchObject({ saving: false, notice: "" });
    expect(editorActivityReducer(failed, { type: "clear-feedback" })).toEqual(
      initialEditorActivityState,
    );
  });
});

describe("accessControlReducer", () => {
  it("ACL取得時のETag revisionを保存の基準にする", () => {
    const loaded = accessControlReducer(initialAccessControlState(1), {
      type: "loaded",
      entries: [],
      revision: 4,
    });
    expect(loaded.revision).toBe(4);
    expect(loaded.status).toBe("ready");
  });

  it("保存中は明示的な状態として扱い、完了後に操作可能へ戻す", () => {
    const loaded = accessControlReducer(initialAccessControlState(1), {
      type: "loaded",
      entries: [],
      revision: 1,
    });
    const saving = accessControlReducer(loaded, { type: "save-started" });
    expect(saving.status).toBe("saving");
    expect(
      accessControlReducer(saving, { type: "saved", revision: 2 }).status,
    ).toBe("ready");
  });

  it("同じissuerとsubjectの権限を重複させず置換する", () => {
    let state = accessControlReducer(initialAccessControlState(1), {
      type: "loaded",
      entries: [
        {
          issuer: "https://id.example.test",
          subject: "reader",
          permission: "read",
        },
      ],
      revision: 1,
    });
    state = accessControlReducer(state, {
      type: "issuer",
      value: "https://id.example.test",
    });
    state = accessControlReducer(state, { type: "subject", value: "reader" });
    state = accessControlReducer(state, {
      type: "permission",
      value: "edit",
    });
    state = accessControlReducer(state, { type: "add" });
    expect(state.entries).toEqual([
      {
        issuer: "https://id.example.test",
        subject: "reader",
        permission: "edit",
      },
    ]);
  });
});
