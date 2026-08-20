import { EditorViewMode } from "./viewMode";

export function EditorViewToolbar({
  mode,
  onModeChange,
  livePreviewEnabled,
  onLivePreviewChange,
}: {
  mode: EditorViewMode;
  onModeChange: (mode: EditorViewMode) => void;
  livePreviewEnabled: boolean;
  onLivePreviewChange: (enabled: boolean) => void;
}) {
  const modes: ReadonlyArray<{ mode: EditorViewMode; label: string }> = [
    { mode: "write", label: "執筆" },
    { mode: "preview", label: "プレビュー" },
  ];
  return (
    <div className="editor-view-toolbar" aria-label="表示設定">
      <div className="editor-view-buttons" role="group" aria-label="表示">
        {modes.map((item) => (
          <button
            className="button-segment"
            key={item.mode}
            type="button"
            aria-pressed={mode === item.mode}
            onClick={() => onModeChange(item.mode)}
          >
            {item.label}
          </button>
        ))}
      </div>
      <div className="editor-view-buttons" role="group" aria-label="執筆の表示">
        <button
          className="button-segment"
          type="button"
          aria-pressed={livePreviewEnabled}
          onClick={() => onLivePreviewChange(!livePreviewEnabled)}
        >
          装飾
        </button>
      </div>
    </div>
  );
}
