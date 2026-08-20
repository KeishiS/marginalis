import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import { AttachmentPanel } from "../src/editor/AttachmentPanel";

const NOTE_ID = "0197c9bc-0000-7000-8000-000000000001";
const ATTACHMENT_ID = "0197c9bc-0000-7000-8000-0000000000a1";

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

test("画像をそのまま送信し、本文へ内部参照を挿入する", async () => {
  document.cookie = "__Host-marginalis_csrf=test-csrf; path=/; Secure";
  const fetchMock = vi
    .fn<typeof fetch>()
    .mockResolvedValueOnce(jsonResponse([]))
    .mockResolvedValueOnce(
      jsonResponse(
        {
          attachment_id: ATTACHMENT_ID,
          file_name: "figure.png",
          media_type: "image/png",
          byte_length: 31,
          sha256: "ab".repeat(32),
          created_at_ms: 10,
          created_by_issuer: "https://id.example.test",
          created_by_subject: "alice",
          source_target: `attachment:${ATTACHMENT_ID}`,
        },
        201,
      ),
    );
  vi.stubGlobal("fetch", fetchMock);
  const onInsert = vi.fn();
  const { container } = render(
    <AttachmentPanel
      apiBase="/marginalis/api/v3"
      noteId={NOTE_ID}
      disabled={false}
      filesToUpload={[]}
      uploadSequence={0}
      onInsert={onInsert}
    />,
  );
  await waitFor(() => expect(fetchMock).toHaveBeenCalledTimes(1));

  const bytes = new Uint8Array([
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 0x0d, 0x49, 0x48,
    0x44, 0x52, 0, 0, 0, 1, 0, 0, 0, 1, 1, 2, 3, 4, 5, 6, 7,
  ]);
  const file = new File([bytes], "figure.png", { type: "image/png" });
  const input = container.querySelector<HTMLInputElement>('input[type="file"]');
  expect(input).not.toBeNull();
  fireEvent.change(input!, { target: { files: [file] } });

  expect(await screen.findByText("figure.png")).toBeInTheDocument();
  expect(fetchMock).toHaveBeenLastCalledWith(
    `/marginalis/api/v3/notes/${NOTE_ID}/attachments?file_name=figure.png`,
    expect.objectContaining({
      method: "POST",
      body: file,
      headers: { "x-csrf-token": "test-csrf" },
    }),
  );
  expect(onInsert).toHaveBeenCalledWith(
    `\n\nimage::attachment:${ATTACHMENT_ID}[]\n`,
  );
});

function jsonResponse(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "content-type": "application/json" },
  });
}
