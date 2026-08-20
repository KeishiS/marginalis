import { ChangeEvent, useEffect, useRef, useState } from "react";

import { Button } from "@/components/ui/button";

import {
  deleteNoteAttachment,
  listNoteAttachments,
  type NoteAttachment,
  uploadNoteAttachment,
} from "../api";
import { toProblem } from "../editorPresentation";

interface AttachmentPanelProps {
  apiBase: string;
  noteId: string | null;
  disabled: boolean;
  filesToUpload: File[];
  uploadSequence: number;
  onInsert: (source: string) => void;
}

export function AttachmentPanel({
  apiBase,
  noteId,
  disabled,
  filesToUpload,
  uploadSequence,
  onInsert,
}: AttachmentPanelProps) {
  const [attachments, setAttachments] = useState<NoteAttachment[]>([]);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const input = useRef<HTMLInputElement>(null);
  const handledUploadSequence = useRef(0);

  useEffect(() => {
    if (!noteId) {
      return;
    }
    const controller = new AbortController();
    listNoteAttachments(apiBase, noteId, controller.signal)
      .then(setAttachments)
      .catch((error: unknown) => {
        if (!controller.signal.aborted) setMessage(toProblem(error).message);
      });
    return () => controller.abort();
  }, [apiBase, noteId]);

  useEffect(() => {
    if (
      busy ||
      filesToUpload.length === 0 ||
      uploadSequence === handledUploadSequence.current
    ) {
      return;
    }
    // 処理中にもう一度dropされた場合は、現在の処理後に新しい番号の分を実行します。
    handledUploadSequence.current = uploadSequence;
    void upload(filesToUpload);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [uploadSequence, busy]);

  async function upload(files: File[]) {
    if (!noteId || files.length === 0 || busy) return;
    setBusy(true);
    setMessage(null);
    try {
      for (const file of files) {
        const uploaded = await uploadNoteAttachment(apiBase, noteId, file);
        setAttachments((current) => [...current, uploaded]);
        onInsert(`\n\nimage::${uploaded.source_target}[]\n`);
      }
    } catch (error: unknown) {
      setMessage(toProblem(error).message);
    } finally {
      setBusy(false);
      if (input.current) input.current.value = "";
    }
  }

  async function remove(attachment: NoteAttachment) {
    if (!noteId || busy) return;
    setBusy(true);
    setMessage(null);
    try {
      await deleteNoteAttachment(apiBase, noteId, attachment.attachment_id);
      setAttachments((current) =>
        current.filter(
          (candidate) => candidate.attachment_id !== attachment.attachment_id,
        ),
      );
    } catch (error: unknown) {
      setMessage(toProblem(error).message);
    } finally {
      setBusy(false);
    }
  }

  function selectFiles(event: ChangeEvent<HTMLInputElement>) {
    void upload(Array.from(event.target.files ?? []));
  }

  return (
    <section
      className="grid gap-3 rounded-md border bg-card p-3"
      aria-labelledby="attachments-heading"
    >
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div>
          <h2 id="attachments-heading" className="m-0 text-sm font-bold">
            添付画像
          </h2>
          <p className="m-0 text-xs text-muted-foreground">
            PNG、JPEG、WebPを選ぶか、本文へドロップしてください。1件8
            MiBまでです。
          </p>
        </div>
        <Button
          type="button"
          variant="outline"
          disabled={disabled || busy || noteId === null}
          onClick={() => input.current?.click()}
        >
          {busy ? "処理しています…" : "画像を追加"}
        </Button>
        <input
          ref={input}
          className="sr-only"
          type="file"
          accept="image/png,image/jpeg,image/webp"
          multiple
          disabled={disabled || busy || noteId === null}
          onChange={selectFiles}
        />
      </div>
      {noteId === null && (
        <p className="m-0 text-sm text-muted-foreground">
          先にノートを保存すると、画像を追加できます。
        </p>
      )}
      {message && (
        <p className="m-0 text-sm text-destructive" role="alert">
          {message}
        </p>
      )}
      {attachments.length > 0 && noteId && (
        <ul className="m-0 grid list-none gap-2 p-0 sm:grid-cols-2">
          {attachments.map((attachment) => (
            <li
              key={attachment.attachment_id}
              className="flex items-center gap-3 rounded border p-2"
            >
              <img
                className="size-14 rounded object-cover"
                src={`${apiBase}/notes/${encodeURIComponent(noteId)}/attachments/${encodeURIComponent(attachment.attachment_id)}/content`}
                alt=""
              />
              <div className="min-w-0 flex-1">
                <p className="m-0 truncate text-sm font-medium">
                  {attachment.file_name}
                </p>
                <p className="m-0 text-xs text-muted-foreground">
                  {formatBytes(attachment.byte_length)}
                </p>
              </div>
              <Button
                type="button"
                variant="ghost"
                disabled={disabled || busy}
                onClick={() => void remove(attachment)}
                aria-label={`${attachment.file_name}を削除`}
              >
                削除
              </Button>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}

function formatBytes(value: number): string {
  if (value < 1024) return `${value} B`;
  if (value < 1024 * 1024) return `${(value / 1024).toFixed(1)} KiB`;
  return `${(value / 1024 / 1024).toFixed(1)} MiB`;
}
