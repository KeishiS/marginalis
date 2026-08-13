import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import { ConfirmationDialog } from "../src/ConfirmationDialog";

afterEach(cleanup);

function dialog(busy: boolean, onCancel = vi.fn()) {
  return (
    <ConfirmationDialog
      eyebrow="Confirm"
      heading="操作を続けますか？"
      description="確認してください。"
      busy={busy}
      problem={null}
      confirmLabel="続ける"
      busyLabel="処理しています…"
      onCancel={onCancel}
      onConfirm={vi.fn()}
    />
  );
}

test("送信中は確認画面へフォーカスを保ち、操作可能になると取り消しへ戻す", () => {
  const onCancel = vi.fn();
  const { rerender } = render(dialog(false, onCancel));
  const cancel = screen.getByRole("button", { name: "取り消す" });
  const confirmation = screen.getByRole("alertdialog");
  expect(cancel).toHaveFocus();

  rerender(dialog(true, onCancel));
  expect(confirmation).toHaveFocus();
  expect(cancel).toBeDisabled();
  expect(
    screen.getByRole("button", { name: "処理しています…" }),
  ).toBeDisabled();

  fireEvent.keyDown(confirmation, { key: "Tab" });
  expect(confirmation).toHaveFocus();
  fireEvent.keyDown(confirmation, { key: "Tab", shiftKey: true });
  expect(confirmation).toHaveFocus();
  fireEvent.keyDown(confirmation, { key: "Escape" });
  expect(onCancel).not.toHaveBeenCalled();

  rerender(dialog(false, onCancel));
  expect(cancel).toHaveFocus();
});
