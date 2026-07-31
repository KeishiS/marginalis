import { EditorViewMode } from "./viewMode";

export function EditorViewToolbar({
  mode,
  requestedMode,
  narrow,
  editorWidth,
  syncScroll,
  onModeChange,
  onEditorWidthChange,
  onSyncScrollChange,
}: {
  mode: EditorViewMode;
  requestedMode: EditorViewMode;
  narrow: boolean;
  editorWidth: number;
  syncScroll: boolean;
  onModeChange: (mode: EditorViewMode) => void;
  onEditorWidthChange: (width: number) => void;
  onSyncScrollChange: (enabled: boolean) => void;
}) {
  const modes: ReadonlyArray<{ mode: EditorViewMode; label: string }> = [
    { mode: "write", label: "執筆" },
    { mode: "split", label: "分割" },
    { mode: "preview", label: "プレビュー" },
  ];
  return (
    <div className="editor-view-toolbar" aria-label="表示設定">
      <div className="editor-view-buttons" role="group" aria-label="表示">
        {modes.map((item) => (
          <button
            className="button button-segment"
            key={item.mode}
            type="button"
            aria-pressed={mode === item.mode}
            disabled={item.mode === "split" && narrow}
            onClick={() => onModeChange(item.mode)}
          >
            {item.label}
          </button>
        ))}
      </div>
      {mode === "split" && (
        <>
          <label className="editor-width-control">
            執筆欄の幅
            <input
              type="range"
              min="30"
              max="70"
              step="5"
              value={editorWidth}
              onChange={(event) =>
                onEditorWidthChange(Number(event.currentTarget.value))
              }
            />
            <output>{editorWidth}%</output>
          </label>
          <label className="scroll-sync-control">
            <input
              type="checkbox"
              checked={syncScroll}
              onChange={(event) =>
                onSyncScrollChange(event.currentTarget.checked)
              }
            />
            相対位置でスクロールを同期
          </label>
          <span className="editor-view-note">
            文書全体に対する位置の割合を合わせるため、見出しや図表の高さによって位置がずれます。
          </span>
        </>
      )}
      {narrow && requestedMode === "split" && (
        <span className="editor-view-note" role="status">
          この画面幅では執筆表示に切り替えています。
        </span>
      )}
    </div>
  );
}
