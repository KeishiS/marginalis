import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import { AsciiDocEditor } from "../src/AsciiDocEditor";

afterEach(cleanup);

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
      styleNonce="test-style-nonce"
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

test("CodeMirrorが生成する基礎CSSへCSP nonceを設定する", () => {
  render(<LabelledEditor value="= 文書" onChange={() => {}} />);

  expect(
    document.head.querySelector("style[nonce='test-style-nonce']"),
  ).toBeTruthy();
});

test("外部から確定した文書を履歴へ加えず同期する", async () => {
  const onChange = vi.fn();
  const { rerender } = render(
    <LabelledEditor value="= 最初" onChange={onChange} />,
  );
  rerender(<LabelledEditor value="= 外部更新" onChange={onChange} />);

  await waitFor(() => expect(editorText()).toBe("= 外部更新"));
});

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
        styleNonce="test-style-nonce"
      />
    </>
  );
}

function editorText(): string {
  return Array.from(document.querySelectorAll(".cm-line"))
    .map((line) => line.textContent ?? "")
    .join("\n");
}
