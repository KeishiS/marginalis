import { ChangeEvent, useState } from "react";

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
      <div className="bibliography-import-launcher">
        <button
          className="button button-secondary"
          type="button"
          onClick={() => void showPanel()}
        >
          CSL-JSONをまとめて取り込む
        </button>
      </div>
    );
  }

  return (
    <section
      className="bibliography-import"
      aria-labelledby="bibliography-import-heading"
    >
      <div className="section-heading-row">
        <div>
          <h2 id="bibliography-import-heading">CSL-JSONの一括取り込み</h2>
          <p>
            ファイルの内容と競合を事前確認してから、一度に保存します。入力にない文献は削除しません。
          </p>
        </div>
        <button
          className="button button-secondary"
          type="button"
          disabled={busy}
          onClick={() => setOpen(false)}
        >
          閉じる
        </button>
      </div>
      <div className="bibliography-import-controls">
        <label>
          取込元
          <select
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
          <label>
            取込元の表示名
            <input
              value={newSourceName}
              maxLength={128}
              disabled={busy || preview !== null}
              onChange={(event) => setNewSourceName(event.target.value)}
              placeholder="例: Zoteroの研究ライブラリー"
            />
          </label>
        )}
        <label>
          CSL-JSONファイル
          <input
            type="file"
            accept="application/json,.json"
            disabled={busy || preview !== null}
            onChange={(event) => void readFile(event)}
          />
        </label>
        {fileName && <p className="field-help">選択中: {fileName}</p>}
        <button
          className="button button-primary"
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
        </button>
      </div>
      {message && (
        <p
          className={failed ? "problem-inline" : "state-message"}
          role={failed ? "alert" : "status"}
        >
          {message}
        </p>
      )}
      {preview && items && (
        <>
          <ol className="bibliography-import-results">
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
          <button
            className="button button-primary"
            type="button"
            disabled={busy || unresolved}
            onClick={() => void applyImport()}
          >
            {busy ? "取り込んでいます…" : "選択した計画を取り込む"}
          </button>
        </>
      )}
    </section>
  );
}
