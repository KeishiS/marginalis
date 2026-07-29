import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { useRef, useState } from "react";
import { afterEach, expect, test, vi } from "vitest";

import {
  AsciiDocEditor,
  type AsciiDocEditorHandle,
} from "../src/AsciiDocEditor";

afterEach(cleanup);

test("入力補助を一つの編集履歴として適用して元に戻せる", async () => {
  render(<EditorHarness />);
  const editor = screen.getByRole("textbox", { name: "AsciiDoc文書" });

  fireEvent.click(screen.getByRole("button", { name: "節を挿入" }));
  await waitFor(() =>
    expect(screen.getByTestId("source")).toHaveTextContent("== 節題= 初期文書"),
  );

  fireEvent.keyDown(editor, { key: "z", ctrlKey: true });
  await waitFor(() =>
    expect(screen.getByTestId("source")).toHaveTextContent("= 初期文書"),
  );
});

test("日本語IMEの変換状態と保存ショートカットを親へ通知する", () => {
  const composition = vi.fn();
  const save = vi.fn();
  render(
    <AsciiDocEditor
      value="= 文書"
      disabled={false}
      labelledBy="test-editor-label"
      onChange={() => {}}
      onCompositionChange={composition}
      onSave={save}
      onScroll={() => {}}
    />,
    {
      wrapper: ({ children }) => (
        <>
          <span id="test-editor-label">AsciiDoc文書</span>
          {children}
        </>
      ),
    },
  );
  const editor = screen.getByRole("textbox", { name: "AsciiDoc文書" });

  fireEvent.keyDown(editor, { key: "s", ctrlKey: true });
  expect(save).toHaveBeenCalledTimes(1);
  fireEvent.compositionStart(editor);
  fireEvent.compositionEnd(editor);
  expect(composition).toHaveBeenNthCalledWith(1, true);
});

test("外部から確定した文書を履歴へ加えず同期する", async () => {
  const onChange = vi.fn();
  const { rerender } = render(
    <LabelledEditor value="= 最初" onChange={onChange} />,
  );
  rerender(<LabelledEditor value="= 外部更新" onChange={onChange} />);

  await waitFor(() => expect(editorText()).toBe("= 外部更新"));
});

function EditorHarness() {
  const [source, setSource] = useState("= 初期文書");
  const editor = useRef<AsciiDocEditorHandle>(null);
  return (
    <>
      <span id="harness-editor-label">AsciiDoc文書</span>
      <AsciiDocEditor
        ref={editor}
        value={source}
        disabled={false}
        labelledBy="harness-editor-label"
        onChange={setSource}
        onCompositionChange={() => {}}
        onSave={() => {}}
        onScroll={() => {}}
      />
      <button
        type="button"
        onClick={() => editor.current?.applyCommand("section")}
      >
        節を挿入
      </button>
      <output data-testid="source">{source}</output>
    </>
  );
}

function LabelledEditor({
  value,
  onChange,
}: {
  value: string;
  onChange: (value: string) => void;
}) {
  return (
    <>
      <span id="external-editor-label">AsciiDoc文書</span>
      <AsciiDocEditor
        value={value}
        disabled={false}
        labelledBy="external-editor-label"
        onChange={onChange}
        onCompositionChange={() => {}}
        onSave={() => {}}
        onScroll={() => {}}
      />
    </>
  );
}

function editorText(): string {
  return Array.from(document.querySelectorAll(".cm-line"))
    .map((line) => line.textContent ?? "")
    .join("\n");
}
