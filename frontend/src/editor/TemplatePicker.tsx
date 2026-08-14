import { useCallback, useState } from "react";

import { ProblemAlert } from "@/components/feedback";

import { NOTE_TEMPLATE_TAG, listNotes, readNote } from "../api";
import { ConfirmationDialog } from "../ConfirmationDialog";
import { useApiResource } from "../useApiResource";

interface TemplatePickerProps {
  apiBase: string;
  disabled: boolean;
  /** 現在の入力が初期状態から変更されているか。適用の前に破棄の確認を挟む。 */
  dirty: boolean;
  onApply: (source: string) => void;
}

/**
 * 新規作成でテンプレートノートを選び、本文を初期内容として適用する部品。
 *
 * テンプレートは「テンプレート」タグの付いた閲覧できるノートで、専用の保存領域を
 * 持たない。候補が無い場合は何も表示せず、従来の空の状態から始める動線を変えない。
 */
export function TemplatePicker({
  apiBase,
  disabled,
  dirty,
  onApply,
}: TemplatePickerProps) {
  const load = useCallback(
    async (signal: AbortSignal) =>
      (await listNotes(apiBase, signal)).filter((note) =>
        note.tags.includes(NOTE_TEMPLATE_TAG),
      ),
    [apiBase],
  );
  const resource = useApiResource(load);
  const templates = resource.status === "ready" ? resource.value : [];
  const [pending, setPending] = useState<string | null>(null);
  const [applying, setApplying] = useState(false);
  const [problem, setProblem] = useState(false);

  async function apply(noteId: string) {
    setApplying(true);
    try {
      const note = await readNote(apiBase, noteId);
      onApply(note.source);
      setProblem(false);
    } catch {
      setProblem(true);
    } finally {
      setApplying(false);
      setPending(null);
    }
  }

  if (templates.length === 0) return null;
  return (
    <div className="flex flex-wrap items-center gap-2">
      <label className="flex items-center gap-2 text-sm font-semibold">
        テンプレートから開始
        <select
          value=""
          disabled={disabled || applying}
          onChange={(event) => {
            const noteId = event.target.value;
            if (!noteId) return;
            if (dirty) {
              setPending(noteId);
            } else {
              void apply(noteId);
            }
          }}
        >
          <option value="">選択してください</option>
          {templates.map((template) => (
            <option key={template.note_id} value={template.note_id}>
              {template.title}
            </option>
          ))}
        </select>
      </label>
      {problem && (
        <ProblemAlert>テンプレートを読み込めませんでした。</ProblemAlert>
      )}
      {pending !== null && (
        <ConfirmationDialog
          eyebrow="テンプレートの適用"
          heading="編集中の内容を置き換えますか"
          description="現在の入力はテンプレートの本文で置き換えられ、元に戻せません。"
          busy={applying}
          problem={null}
          confirmLabel="置き換える"
          busyLabel="適用しています…"
          destructive
          onCancel={() => setPending(null)}
          onConfirm={() => void apply(pending)}
        />
      )}
    </div>
  );
}
