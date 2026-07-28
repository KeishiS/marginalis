import { render, screen } from "@testing-library/react";
import { expect, test } from "vitest";

import { EditorApplication } from "../src/EditorApplication";

test("編集画面の見出しを表示する", () => {
  render(<EditorApplication />);

  expect(
    screen.getByRole("heading", { name: "ノートの編集" }),
  ).toBeInTheDocument();
});
