import {
  BibliographyImportDecisionAction,
  BibliographyImportEntry,
} from "../api";

const CLASSIFICATION_LABELS: Record<
  BibliographyImportEntry["classification"],
  string
> = {
  create: "新規作成",
  update_from_external: "外部側の更新を反映",
  unchanged: "変更なし",
  keep_local: "Marginalis側を保持",
  conflict: "双方に変更あり",
  duplicate_candidate: "重複候補あり",
  rejected: "取込不可",
};

const REJECTION_LABELS: Record<string, string> = {
  item_not_object: "CSL-JSONの項目がオブジェクトではありません。",
  missing_id: "idがありません。",
  invalid_id: "idに使用できない文字があるか、長すぎます。",
  missing_type: "typeがありません。",
  invalid_type: "typeが空か、使用できない文字があるか、長すぎます。",
  invalid_json: "JSONとして解釈できません。",
  item_too_large: "一項目の大きさが上限を超えています。",
  item_too_deep: "JSONの入れ子が深すぎます。",
  string_too_long: "文字列が長すぎます。",
  invalid_object_key: "項目名が空か、使用できない文字があるか、長すぎます。",
  duplicate_external_item_id: "同じファイル内でidが重複しています。",
  stored_link_target_missing: "保存済みの対応情報が壊れています。",
};

const MATCH_LABELS: Record<string, string> = {
  citation_key: "citation key",
  doi: "DOI",
  isbn: "ISBN",
  pmid: "PMID",
  pmcid: "PMCID",
  url: "URL",
  title: "題名",
};

interface BibliographyImportDecisionState {
  action: BibliographyImportDecisionAction | "";
  candidateItemId: string | null;
}

export function BibliographyImportEntryDecision({
  entry,
  externalCslJson,
  decision,
  disabled,
  onChange,
}: {
  entry: BibliographyImportEntry;
  externalCslJson: unknown;
  decision: BibliographyImportDecisionState;
  disabled: boolean;
  onChange: (decision: BibliographyImportDecisionState) => void;
}) {
  const value = decision.candidateItemId
    ? `${decision.action}:${decision.candidateItemId}`
    : decision.action;
  const rejection = entry.rejection_code
    ? (REJECTION_LABELS[entry.rejection_code] ?? "項目を取り込めません。")
    : null;

  return (
    <li className="grid gap-3 rounded-sm border bg-card p-3">
      <div className="flex flex-wrap gap-2">
        <strong>
          {entry.position + 1}. {entry.citation_key ?? "識別子なし"}
        </strong>
        <span className="text-muted-foreground">
          {CLASSIFICATION_LABELS[entry.classification]}
        </span>
        {rejection && (
          <span className="text-sm text-destructive">{rejection}</span>
        )}
      </div>
      <label className="grid gap-1 text-sm font-semibold">
        処理
        <select
          className="w-full"
          aria-label={`${entry.position + 1}件目の処理`}
          value={value}
          disabled={disabled}
          onChange={(event) => {
            const [action, candidateItemId] = event.target.value.split(":", 2);
            onChange({
              action: action as BibliographyImportDecisionAction | "",
              candidateItemId: candidateItemId ?? null,
            });
          }}
        >
          {(entry.classification === "conflict" ||
            entry.classification === "duplicate_candidate") && (
            <option value="">選択してください</option>
          )}
          {!["conflict", "duplicate_candidate", "rejected"].includes(
            entry.classification,
          ) && <option value="apply_suggested">予定どおり処理</option>}
          {entry.classification === "conflict" && (
            <>
              <option value="keep_local">Marginalis側を保持</option>
              <option value="use_external">外部側を採用</option>
            </>
          )}
          {entry.classification === "duplicate_candidate" && (
            <>
              {!entry.candidates.some((candidate) =>
                candidate.matched_by.includes("citation_key"),
              ) && (
                <option value="create_separate">別の文献として新規作成</option>
              )}
              {entry.candidates.flatMap((candidate) => [
                <option
                  key={`keep:${candidate.item_id}`}
                  value={`link_existing_keep_local:${candidate.item_id}`}
                >
                  {candidate.citation_key}へ対応し、登録済み情報を保持
                </option>,
                <option
                  key={`use:${candidate.item_id}`}
                  value={`link_existing_use_external:${candidate.item_id}`}
                >
                  {candidate.citation_key}へ対応し、外部側を採用
                </option>,
              ])}
            </>
          )}
          <option value="exclude">今回の計画から除外</option>
        </select>
      </label>
      <div className="grid gap-2 min-[60rem]:grid-cols-[repeat(auto-fit,minmax(18rem,1fr))]">
        <ComparisonDetails label="外部側のCSL-JSON" value={externalCslJson} />
        {entry.current_csl_json && (
          <ComparisonDetails
            label="Marginalis側の現在値"
            value={entry.current_csl_json}
          />
        )}
      </div>
      {entry.candidates.length > 0 && (
        <ul className="m-0 list-none p-0 text-sm text-muted-foreground">
          {entry.candidates.map((candidate) => (
            <li key={candidate.item_id}>
              {candidate.citation_key}
              {candidate.title ? ` — ${candidate.title}` : ""}（一致:{" "}
              {candidate.matched_by
                .map((field) => MATCH_LABELS[field] ?? field)
                .join(", ")}
              ）
            </li>
          ))}
        </ul>
      )}
    </li>
  );
}

/** 比較用のCSL-JSONを、折りたたみで場所を取らずに示す。 */
function ComparisonDetails({
  label,
  value,
}: {
  label: string;
  value: unknown;
}) {
  return (
    <details className="min-w-0 rounded-sm border bg-muted p-2">
      <summary className="cursor-pointer text-sm font-semibold">
        {label}
      </summary>
      <pre className="mt-2 mb-0 max-h-80 overflow-auto text-xs whitespace-pre-wrap [overflow-wrap:anywhere]">
        {JSON.stringify(value, null, 2)}
      </pre>
    </details>
  );
}
