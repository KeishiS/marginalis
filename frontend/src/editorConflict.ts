export interface ConflictLine {
  line: string;
  status: string;
  changed: boolean;
  editingStarted: string | null;
  editing: string | null;
  current: string | null;
}

interface AlignedLines {
  insertions: string[][];
  matches: Array<string | null>;
}

export function alignThreeVersions(
  editingStarted: string,
  editing: string,
  current: string,
): ConflictLine[] {
  const baseline = splitLines(editingStarted);
  const editingAligned = alignToBaseline(baseline, splitLines(editing));
  const currentAligned = alignToBaseline(baseline, splitLines(current));
  const rows: ConflictLine[] = [];
  for (
    let baselineIndex = 0;
    baselineIndex <= baseline.length;
    baselineIndex++
  ) {
    const editingInsertions = editingAligned.insertions[baselineIndex] ?? [];
    const currentInsertions = currentAligned.insertions[baselineIndex] ?? [];
    const insertionCount = Math.max(
      editingInsertions.length,
      currentInsertions.length,
    );
    for (
      let insertionIndex = 0;
      insertionIndex < insertionCount;
      insertionIndex++
    ) {
      const editingLine = editingInsertions[insertionIndex] ?? null;
      const currentLine = currentInsertions[insertionIndex] ?? null;
      rows.push({
        line: "追加",
        status:
          editingLine !== null && currentLine !== null
            ? "編集中と現在の内容に追加"
            : editingLine !== null
              ? "編集中に追加"
              : "現在の内容に追加",
        changed: true,
        editingStarted: null,
        editing: editingLine,
        current: currentLine,
      });
    }
    if (baselineIndex === baseline.length) {
      continue;
    }
    const editingLine = editingAligned.matches[baselineIndex] ?? null;
    const currentLine = currentAligned.matches[baselineIndex] ?? null;
    const deletions = [
      editingLine === null ? "編集中から削除" : "",
      currentLine === null ? "現在の内容から削除" : "",
    ].filter(Boolean);
    rows.push({
      line: String(baselineIndex + 1),
      status: deletions.length > 0 ? deletions.join("、") : "変更なし",
      changed: deletions.length > 0,
      editingStarted: baseline[baselineIndex] ?? "",
      editing: editingLine,
      current: currentLine,
    });
  }
  return rows;
}

function splitLines(value: string): string[] {
  return value.split(/\r\n|\r|\n/);
}

function alignToBaseline(baseline: string[], variant: string[]): AlignedLines {
  if (baseline.length * variant.length > 250_000) {
    return alignLargeDocument(baseline, variant);
  }
  const lengths = Array.from({ length: baseline.length + 1 }, () =>
    Array<number>(variant.length + 1).fill(0),
  );
  for (let left = baseline.length - 1; left >= 0; left--) {
    for (let right = variant.length - 1; right >= 0; right--) {
      lengths[left]![right] =
        baseline[left] === variant[right]
          ? (lengths[left + 1]?.[right + 1] ?? 0) + 1
          : Math.max(
              lengths[left + 1]?.[right] ?? 0,
              lengths[left]?.[right + 1] ?? 0,
            );
    }
  }
  const insertions = Array.from(
    { length: baseline.length + 1 },
    () => [] as string[],
  );
  const matches = Array<string | null>(baseline.length).fill(null);
  let left = 0;
  let right = 0;
  while (left < baseline.length && right < variant.length) {
    if (baseline[left] === variant[right]) {
      matches[left] = variant[right] ?? "";
      left++;
      right++;
    } else if (
      (lengths[left]?.[right + 1] ?? 0) >= (lengths[left + 1]?.[right] ?? 0)
    ) {
      insertions[left]?.push(variant[right] ?? "");
      right++;
    } else {
      left++;
    }
  }
  while (right < variant.length) {
    insertions[baseline.length]?.push(variant[right] ?? "");
    right++;
  }
  return { insertions, matches };
}

function alignLargeDocument(
  baseline: string[],
  variant: string[],
): AlignedLines {
  const insertions = Array.from(
    { length: baseline.length + 1 },
    () => [] as string[],
  );
  const matches = baseline.map((line, index) =>
    variant[index] === line ? line : null,
  );
  for (let index = 0; index < variant.length; index++) {
    if (matches[index] === null || index >= baseline.length) {
      insertions[Math.min(index, baseline.length)]?.push(variant[index] ?? "");
    }
  }
  return { insertions, matches };
}
