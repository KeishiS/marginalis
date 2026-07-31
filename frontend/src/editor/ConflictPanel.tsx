import { useEffect, useRef } from "react";

import { EditorForm as FormState } from "../editorState";
import { alignThreeVersions } from "../editorConflict";

export function ConflictPanel({
  editingStarted,
  editing,
  current,
  currentRevision,
  onUseCurrentRevision,
}: {
  editingStarted: FormState;
  editing: FormState;
  current: FormState;
  currentRevision: number;
  onUseCurrentRevision: () => void;
}) {
  const heading = useRef<HTMLHeadingElement>(null);
  useEffect(() => heading.current?.focus(), []);
  return (
    <section className="conflict-panel" aria-labelledby="conflict-heading">
      <h2 id="conflict-heading" ref={heading} tabIndex={-1}>
        更新内容の競合
      </h2>
      <p>
        編集中の内容は維持されています。三つの内容を比較し、必要な修正を行ってください。
      </p>
      <h3>AsciiDoc文書の行単位比較</h3>
      <BodyConflictTable
        editingStarted={editingStarted.source}
        editing={editing.source}
        current={current.source}
      />
      <button
        className="button button-secondary"
        type="button"
        onClick={onUseCurrentRevision}
      >
        更新番号{currentRevision}を編集の基準にする
      </button>
      <p>
        この操作では保存しません。比較後にフォームの「保存」を選んでください。
      </p>
    </section>
  );
}

function BodyConflictTable({
  editingStarted,
  editing,
  current,
}: {
  editingStarted: string;
  editing: string;
  current: string;
}) {
  const rows = alignThreeVersions(editingStarted, editing, current);
  return (
    <div
      className="conflict-body-scroll"
      tabIndex={0}
      aria-label="本文比較表のスクロール領域"
    >
      <table className="conflict-body">
        <caption>本文の行単位比較</caption>
        <thead>
          <tr>
            <th scope="col">行</th>
            <th scope="col">状態</th>
            <th scope="col">編集開始時点</th>
            <th scope="col">編集中</th>
            <th scope="col">現在保存されている内容</th>
          </tr>
        </thead>
        <tbody>
          {rows.map((row, index) => (
            <tr className={row.changed ? "changed" : undefined} key={index}>
              <th scope="row">{row.line}</th>
              <td className="change-status">{row.status}</td>
              {[row.editingStarted, row.editing, row.current].map(
                (value, column) => (
                  <td key={column}>
                    <code>{value ?? "\u00a0"}</code>
                  </td>
                ),
              )}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
