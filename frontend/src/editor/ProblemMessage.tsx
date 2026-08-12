import { NoteDiagnostic, Problem } from "../api";
import {
  canSelectDiagnostic,
  diagnosticLocation,
  diagnosticMessage,
  diagnosticSeverityLabel,
  problemMessage,
} from "../editorPresentation";

export function ProblemMessage({
  problem,
  heading,
  headingId,
  onSelectDiagnostic,
}: {
  problem: Problem;
  heading: string;
  headingId: string;
  onSelectDiagnostic?: (diagnostic: NoteDiagnostic) => void;
}) {
  return (
    <section className="problem" aria-labelledby={headingId} role="alert">
      <h2 id={headingId}>{heading}</h2>
      <p>{problemMessage(problem)}</p>
      {problem.diagnostics && problem.diagnostics.length > 0 && (
        <ul>
          {problem.diagnostics.map((diagnostic, index) => (
            <li key={`${diagnostic.code}-${index}`}>
              <span className="diagnostic-severity">
                {diagnosticSeverityLabel(diagnostic.severity)}:{" "}
              </span>
              {diagnosticLocation(diagnostic)}
              {diagnosticMessage(diagnostic.code, diagnostic.message)}{" "}
              {canSelectDiagnostic(diagnostic) && onSelectDiagnostic && (
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
  );
}
