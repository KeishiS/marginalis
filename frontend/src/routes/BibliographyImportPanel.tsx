import { ChangeEvent, useState } from "react";

import { ProblemAlert, StatusMessage } from "@/components/feedback";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";

import {
  applyBibliographyImport,
  BibliographyImportDecision,
  BibliographyImportDecisionAction,
  BibliographyImportEntry,
  BibliographyImportPreview,
  BibliographyImportSource,
  BibliographyImportSourceInput,
  listBibliographyImportSources,
  previewBibliographyImport,
} from "../api";
import { BibliographyImportEntryDecision } from "./BibliographyImportEntryDecision";

const MAX_FILE_BYTES = 8 * 1024 * 1024;

interface BibliographyImportDecisionState {
  action: BibliographyImportDecisionAction | "";
  candidateItemId: string | null;
}

function initialBibliographyImportDecision(
  entry: BibliographyImportEntry,
): BibliographyImportDecisionState {
  switch (entry.classification) {
    case "create":
    case "update_from_external":
    case "unchanged":
    case "keep_local":
      return { action: "apply_suggested", candidateItemId: null };
    case "rejected":
      return { action: "exclude", candidateItemId: null };
    case "conflict":
    case "duplicate_candidate":
      return { action: "", candidateItemId: null };
  }
}

function sourceInput(
  selectedSourceId: string,
  newSourceName: string,
): BibliographyImportSourceInput {
  return selectedSourceId === "new"
    ? { kind: "new", display_name: newSourceName }
    : { kind: "existing", source_id: selectedSourceId };
}

export function BibliographyImportPanel({
  apiBase,
  onApplied,
}: {
  apiBase: string;
  onApplied: () => Promise<void>;
}) {
  const [open, setOpen] = useState(false);
  const [sources, setSources] = useState<BibliographyImportSource[]>([]);
  const [selectedSourceId, setSelectedSourceId] = useState("new");
  const [newSourceName, setNewSourceName] = useState("");
  const [items, setItems] = useState<unknown[] | null>(null);
  const [fileName, setFileName] = useState("");
  const [preview, setPreview] = useState<BibliographyImportPreview | null>(
    null,
  );
  const [decisions, setDecisions] = useState<
    Record<number, BibliographyImportDecisionState>
  >({});
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState("");
  const [failed, setFailed] = useState(false);

  async function showPanel() {
    setOpen(true);
    setBusy(true);
    try {
      setSources(await listBibliographyImportSources(apiBase));
      setMessage("");
    } catch {
      setFailed(true);
      setMessage("取込元を読み込めませんでした。");
    } finally {
      setBusy(false);
    }
  }

  async function readFile(event: ChangeEvent<HTMLInputElement>) {
    const file = event.target.files?.[0];
    setPreview(null);
    setItems(null);
    setFileName("");
    if (!file) return;
    try {
      if (file.size > MAX_FILE_BYTES) throw new Error("too large");
      const value: unknown = JSON.parse(await file.text());
      if (!Array.isArray(value) || value.length === 0) {
        throw new Error("invalid array");
      }
      setItems(value);
      setFileName(file.name);
      setFailed(false);
      setMessage(
        `${value.length}件を読み込みました。事前確認を実行してください。`,
      );
    } catch {
      event.target.value = "";
      setFailed(true);
      setMessage(
        "ファイルを読み込めませんでした。8 MiB以下のCSL-JSON項目配列を選んでください。",
      );
    }
  }

  async function runPreview() {
    if (!items || busy) return;
    setBusy(true);
    try {
      const result = await previewBibliographyImport(
        apiBase,
        sourceInput(selectedSourceId, newSourceName),
        items,
      );
      setPreview(result);
      setDecisions(
        Object.fromEntries(
          result.entries.map((entry) => [
            entry.position,
            initialBibliographyImportDecision(entry),
          ]),
        ),
      );
      setFailed(false);
      setMessage("事前確認が完了しました。分類と選択内容を確認してください。");
    } catch {
      setFailed(true);
      setMessage(
        "事前確認に失敗しました。取込元とファイルを確認してください。",
      );
    } finally {
      setBusy(false);
    }
  }

  const unresolved =
    preview === null ||
    preview.entries.some((entry) => !decisions[entry.position]?.action) ||
    preview.entries.every(
      (entry) => decisions[entry.position]?.action === "exclude",
    );

  async function applyImport() {
    if (!items || !preview || unresolved || busy) return;
    const selectedDecisions: BibliographyImportDecision[] = preview.entries.map(
      (entry) => ({
        position: entry.position,
        action: decisions[entry.position]
          .action as BibliographyImportDecisionAction,
        candidate_item_id: decisions[entry.position].candidateItemId,
      }),
    );
    setBusy(true);
    try {
      const result = await applyBibliographyImport(
        apiBase,
        sourceInput(selectedSourceId, newSourceName),
        items,
        preview.preview_token,
        selectedDecisions,
      );
      setFailed(false);
      setMessage(
        `取り込みました。新規${result.created}件、更新${result.updated}件、保持${result.kept}件、除外${result.excluded}件です。`,
      );
      setPreview(null);
      setItems(null);
      setFileName("");
      setSources(await listBibliographyImportSources(apiBase));
      setSelectedSourceId(result.source_id);
      await onApplied();
    } catch {
      setFailed(true);
      setMessage(
        "取り込めませんでした。保存内容が変わった可能性があるため、もう一度事前確認してください。",
      );
    } finally {
      setBusy(false);
    }
  }

  if (!open) {
    return (
      <div className="my-4 mb-6">
        <Button
          variant="outline"
          type="button"
          onClick={() => void showPanel()}
        >
          CSL-JSONをまとめて取り込む
        </Button>
      </div>
    );
  }

  return (
    <section
      className="grid gap-4 rounded-md border bg-muted p-5"
      aria-labelledby="bibliography-import-heading"
    >
      <div className="flex items-start justify-between gap-4">
        <div>
          <h2 id="bibliography-import-heading">CSL-JSONの一括取り込み</h2>
          <p>
            ファイルの内容と競合を事前確認してから、一度に保存します。入力にない文献は削除しません。
          </p>
        </div>
        <Button
          variant="outline"
          type="button"
          disabled={busy}
          onClick={() => setOpen(false)}
        >
          閉じる
        </Button>
      </div>
      <div className="grid items-end gap-3 min-[60rem]:grid-cols-[repeat(auto-fit,minmax(16rem,1fr))]">
        <label className="grid gap-1 text-sm font-semibold">
          取込元
          <select
            className="w-full"
            value={selectedSourceId}
            disabled={busy || preview !== null}
            onChange={(event) => setSelectedSourceId(event.target.value)}
          >
            <option value="new">新しい取込元</option>
            {sources.map((source) => (
              <option key={source.source_id} value={source.source_id}>
                {source.display_name}
              </option>
            ))}
          </select>
        </label>
        {selectedSourceId === "new" && (
          <label className="grid gap-1 text-sm font-semibold">
            取込元の表示名
            <Input
              value={newSourceName}
              maxLength={128}
              disabled={busy || preview !== null}
              onChange={(event) => setNewSourceName(event.target.value)}
              placeholder="例: Zoteroの研究ライブラリー"
            />
          </label>
        )}
        <label className="grid gap-1 text-sm font-semibold">
          CSL-JSONファイル
          <Input
            type="file"
            accept="application/json,.json"
            disabled={busy || preview !== null}
            onChange={(event) => void readFile(event)}
          />
        </label>
        {fileName && (
          <p className="m-0 text-sm text-muted-foreground">
            選択中: {fileName}
          </p>
        )}
        <Button
          type="button"
          disabled={
            busy ||
            items === null ||
            preview !== null ||
            (selectedSourceId === "new" && !newSourceName.trim())
          }
          onClick={() => void runPreview()}
        >
          {busy ? "確認しています…" : "事前確認"}
        </Button>
      </div>
      {message &&
        (failed ? (
          <ProblemAlert>{message}</ProblemAlert>
        ) : (
          <StatusMessage>{message}</StatusMessage>
        ))}
      {preview && items && (
        <>
          <ol className="m-0 grid list-decimal gap-3 pl-6">
            {preview.entries.map((entry) => (
              <BibliographyImportEntryDecision
                key={entry.position}
                entry={entry}
                externalCslJson={items[entry.position]}
                decision={decisions[entry.position]}
                disabled={busy}
                onChange={(decision) =>
                  setDecisions((current) => ({
                    ...current,
                    [entry.position]: decision,
                  }))
                }
              />
            ))}
          </ol>
          <Button
            type="button"
            disabled={busy || unresolved}
            onClick={() => void applyImport()}
          >
            {busy ? "取り込んでいます…" : "選択した計画を取り込む"}
          </Button>
        </>
      )}
    </section>
  );
}
