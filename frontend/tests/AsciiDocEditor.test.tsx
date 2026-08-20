import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import type { ComponentProps } from "react";
import { afterEach, expect, test, vi } from "vitest";

import { AsciiDocEditor } from "../src/AsciiDocEditor";

afterEach(cleanup);

test("日本語IMEの変換状態と保存ショートカットを親へ通知する", () => {
  const composition = vi.fn();
  const save = vi.fn();
  render(
    <AsciiDocEditor
      value="= 文書"
      diagnostics={[]}
      spans={null}
      mathMacros={[]}
      livePreviewEnabled={false}
      disabled={false}
      labelledBy="test-editor-label"
      onChange={() => {}}
      onCompositionChange={composition}
      onSave={save}
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

test("診断箇所へ波線を引きF8で移動して、修正時に取り除く", async () => {
  const source = "= 題名\n\n結果はxref:note:id[参照]です。";
  const start = new TextEncoder().encode("= 題名\n\n結果は").length;
  const diagnostic = {
    code: "macro-boundary",
    severity: "warning" as const,
    target: { field: "source" as const },
    span: { start, end: start + 4, unit: "utf8_byte" as const },
    message: "a space is required before the inline macro",
  };
  const { rerender } = render(
    <LabelledEditor
      value={source}
      diagnostics={[diagnostic]}
      onChange={() => {}}
    />,
  );

  await waitFor(() => {
    expect(document.querySelector(".cm-lintRange-warning")).toHaveTextContent(
      "xref",
    );
  });
  fireEvent.keyDown(screen.getByRole("textbox", { name: "AsciiDoc文書" }), {
    key: "F8",
  });
  expect(window.getSelection()?.toString()).toBe("xref");

  rerender(
    <LabelledEditor value={source} diagnostics={[]} onChange={() => {}} />,
  );
  await waitFor(() =>
    expect(document.querySelector(".cm-lintRange-warning")).toBeNull(),
  );
});

function LabelledEditor({
  value,
  diagnostics = [],
  onChange,
}: {
  value: string;
  diagnostics?: ComponentProps<typeof AsciiDocEditor>["diagnostics"];
  onChange: (value: string) => void;
}) {
  return (
    <>
      <span id="external-editor-label">AsciiDoc文書</span>
      <AsciiDocEditor
        value={value}
        diagnostics={diagnostics}
        spans={null}
        mathMacros={[]}
        livePreviewEnabled={false}
        disabled={false}
        labelledBy="external-editor-label"
        onChange={onChange}
        onCompositionChange={() => {}}
        onSave={() => {}}
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
