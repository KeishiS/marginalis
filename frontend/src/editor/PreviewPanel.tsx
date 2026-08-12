import { MathMacro, NoteDiagnostic, Problem } from "../api";
import { RenderedContent } from "../RenderedContent";
import {
  canSelectDiagnostic,
  diagnosticMessage,
  diagnosticSeverityLabel,
  diagnosticLocation,
  problemMessage,
} from "../editorPresentation";

export function PreviewPanel({
  active,
  html,
  diagnostics,
  loading,
  mathMacros,
  problem,
  onSelectDiagnostic,
  styleNonce,
}: {
  active: boolean;
  html: string;
  diagnostics: NoteDiagnostic[];
  loading: boolean;
  mathMacros: MathMacro[];
  problem: Problem | null;
  onSelectDiagnostic: (diagnostic: NoteDiagnostic) => void;
  styleNonce: string;
}) {
  const externalDiagnostics = diagnostics.filter(
    (diagnostic) => !canSelectDiagnostic(diagnostic),
  );
  return (
    <section className="preview-panel" aria-labelledby="preview-heading">
      <div className="preview-heading">
        <h2 id="preview-heading">プレビュー</h2>
        <span role="status">
          {loading
            ? "更新しています…"
            : problem && html
              ? "最後に成功したプレビューを表示しています。"
              : problem
                ? "更新に失敗しました。"
                : "最新です。"}
        </span>
      </div>
      {problem && (
        <section
          className="problem"
          aria-labelledby="preview-problem-heading"
          role="alert"
        >
          <h3 id="preview-problem-heading">プレビューできませんでした</h3>
          <p>{problemMessage(problem)}</p>
          {problem.diagnostics && (
            <ul>
              {problem.diagnostics.map((diagnostic, index) => (
                <li key={`${diagnostic.code}-${index}`}>
                  <span className="diagnostic-severity">
                    {diagnosticSeverityLabel(diagnostic.severity)}:{" "}
                  </span>
                  {diagnosticLocation(diagnostic)}
                  {diagnosticMessage(diagnostic.code, diagnostic.message)}{" "}
                  {canSelectDiagnostic(diagnostic) && (
                    <button
                      type="button"
                      className="diagnostic-link"
                      onClick={() => onSelectDiagnostic(diagnostic)}
                    >
                      入力位置へ移動
                    </button>
                  )}
                </li>
              ))}
            </ul>
          )}
        </section>
      )}
      {!problem && externalDiagnostics.length > 0 && (
        <section
          className="warnings"
          aria-labelledby="preview-diagnostics-heading"
        >
          <h3 id="preview-diagnostics-heading">入力時の診断</h3>
          <ul>
            {externalDiagnostics.map((diagnostic, index) => (
              <li key={`${diagnostic.code}-${index}`}>
                <span className="diagnostic-severity">
                  {diagnosticSeverityLabel(diagnostic.severity)}:{" "}
                </span>
                {diagnosticLocation(diagnostic)}
                {diagnosticMessage(diagnostic.code, diagnostic.message)}{" "}
                {canSelectDiagnostic(diagnostic) && (
                  <button
                    type="button"
                    className="diagnostic-link"
                    onClick={() => onSelectDiagnostic(diagnostic)}
                  >
                    入力位置へ移動
                  </button>
                )}
              </li>
            ))}
          </ul>
        </section>
      )}
      {html && (
        <SafePreview
          active={active}
          html={html}
          mathMacros={mathMacros}
          styleNonce={styleNonce}
        />
      )}
      {!html && !loading && !problem && <p>プレビューはありません。</p>}
    </section>
  );
}

function SafePreview({
  active,
  html,
  mathMacros,
  styleNonce,
}: {
  active: boolean;
  html: string;
  mathMacros: MathMacro[];
  styleNonce: string;
}) {
  // 同じ保存規則とRenderPolicyを通ったサーバー生成HTMLだけを受け取る。
  return (
    <RenderedContent
      active={active}
      html={html}
      mathMacros={mathMacros}
      preview
      styleNonce={styleNonce}
    />
  );
}
