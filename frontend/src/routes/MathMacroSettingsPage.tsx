import { FormEvent, useEffect, useState } from "react";

import { ProblemAlert, StatusMessage } from "@/components/feedback";
import { PageHeader } from "@/components/PageHeader";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";

import {
  ApiError,
  ApplicationConfig,
  MathMacro,
  readMathMacros,
  replaceMathMacros,
} from "../api";
import {
  mathMacroBytes,
  MAX_MATH_MACROS,
  MAX_MATH_MACRO_ARGUMENTS,
  MAX_MATH_MACRO_NAME_CHARACTERS,
  MAX_MATH_MACRO_TOTAL_BYTES,
  validateMathMacros,
} from "../mathMacroState";

const EXAMPLES: MathMacro[] = [
  {
    name: "argmax",
    replacement: "\\operatorname*{arg\\,max}",
    argument_count: 0,
  },
  { name: "bm", replacement: "\\boldsymbol{#1}", argument_count: 1 },
];

export function MathMacroSettingsPage({
  config,
}: {
  config: ApplicationConfig;
}) {
  const [macros, setMacros] = useState<MathMacro[] | null>(null);
  const [revision, setRevision] = useState(0);
  const [saving, setSaving] = useState(false);
  const [message, setMessage] = useState("");
  const [failed, setFailed] = useState(false);
  const totalBytes = macros === null ? 0 : mathMacroBytes(macros);
  const validationProblem = macros === null ? null : validateMathMacros(macros);

  useEffect(() => {
    const controller = new AbortController();
    readMathMacros(config.apiBase, controller.signal)
      .then((settings) => {
        if (!controller.signal.aborted) {
          setMacros(settings.macros);
          setRevision(settings.revision);
        }
      })
      .catch(() => {
        if (!controller.signal.aborted) {
          setFailed(true);
          setMessage("数式マクロの設定を読み込めませんでした。");
        }
      });
    return () => controller.abort();
  }, [config.apiBase]);

  function update(index: number, changes: Partial<MathMacro>) {
    setMacros(
      (current) =>
        current?.map((macro, itemIndex) =>
          itemIndex === index ? { ...macro, ...changes } : macro,
        ) ?? null,
    );
    setMessage("");
  }

  function add(
    macro: MathMacro = { name: "", replacement: "", argument_count: 0 },
  ) {
    if ((macros?.length ?? 0) >= MAX_MATH_MACROS) return;
    setMacros((current) => [...(current ?? []), macro]);
    setMessage("");
  }

  function addExample(example: MathMacro) {
    setMacros((current) => {
      const values = current ?? [];
      return values.length >= MAX_MATH_MACROS ||
        values.some((macro) => macro.name === example.name)
        ? values
        : [...values, example];
    });
    setMessage("");
  }

  function remove(index: number) {
    setMacros(
      (current) =>
        current?.filter((_, itemIndex) => itemIndex !== index) ?? null,
    );
    setMessage("");
  }

  async function save(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (macros === null || saving || validationProblem !== null) return;
    setSaving(true);
    setMessage("");
    try {
      const saved = await replaceMathMacros(config.apiBase, {
        macros,
        revision,
      });
      setMacros(saved.macros);
      setRevision(saved.revision);
      setFailed(false);
      setMessage("数式マクロを保存しました。");
    } catch (error: unknown) {
      setFailed(true);
      setMessage(saveFailureMessage(error));
    } finally {
      setSaving(false);
    }
  }

  return (
    <section className="grid gap-6">
      <PageHeader
        eyebrow="Settings"
        title="数式マクロ"
        description="所有するノートで繰り返し使うMathJaxコマンドを定義します。共有されたノートにも、ノート所有者の設定が適用されます。"
      />
      {macros === null && !message ? (
        <StatusMessage>数式マクロを読み込んでいます。</StatusMessage>
      ) : macros === null ? (
        <ProblemAlert>{message}</ProblemAlert>
      ) : (
        <form className="grid gap-4" onSubmit={save}>
          <div
            className="flex flex-wrap items-center gap-3"
            aria-label="定義例"
          >
            <span className="font-bold">定義例を追加</span>
            {EXAMPLES.map((example) => (
              <Button
                key={example.name}
                variant="outline"
                size="sm"
                type="button"
                onClick={() => addExample(example)}
              >
                <code>\{example.name}</code>
              </Button>
            ))}
          </div>
          <p className="m-0 text-sm text-muted-foreground">
            コマンド名には先頭の <code>\</code> を含めません。置換内容では引数を{" "}
            <code>#1</code> から <code>#{MAX_MATH_MACRO_ARGUMENTS}</code>{" "}
            で参照できます。最大
            {MAX_MATH_MACROS}
            件、コマンド名と置換内容の合計は
            {MAX_MATH_MACRO_TOTAL_BYTES / 1024} KiBまでです。 コマンド名の{" "}
            <code>def</code> は使用できません。置換内容の波括弧を対応させ、
            <code>%</code> は <code>\%</code> と記述してください。
          </p>
          <p className="m-0 text-sm text-muted-foreground" role="status">
            {macros.length} / {MAX_MATH_MACROS}件、
            {totalBytes.toLocaleString()} /{" "}
            {MAX_MATH_MACRO_TOTAL_BYTES.toLocaleString()}バイト
          </p>
          <div className="grid gap-4">
            {macros.map((macro, index) => (
              <fieldset
                key={index}
                className="m-0 flex flex-wrap items-end gap-3 rounded-md border bg-card p-4"
              >
                <legend className="px-1 text-sm font-bold">
                  マクロ {index + 1}
                </legend>
                <label className="grid gap-2 text-sm font-semibold">
                  コマンド名
                  <Input
                    required
                    pattern="[A-Za-z]+"
                    maxLength={MAX_MATH_MACRO_NAME_CHARACTERS}
                    value={macro.name}
                    onChange={(event) =>
                      update(index, { name: event.target.value })
                    }
                  />
                </label>
                <label className="grid gap-2 text-sm font-semibold">
                  引数の数
                  <Input
                    className="w-24"
                    required
                    type="number"
                    min={0}
                    max={MAX_MATH_MACRO_ARGUMENTS}
                    value={macro.argument_count}
                    onChange={(event) =>
                      update(index, {
                        argument_count: Number(event.target.value),
                      })
                    }
                  />
                </label>
                <label className="grid flex-[1_1_20rem] gap-2 text-sm font-semibold">
                  置換内容
                  <Input
                    required
                    value={macro.replacement}
                    onChange={(event) =>
                      update(index, { replacement: event.target.value })
                    }
                  />
                </label>
                <Button
                  variant="outline"
                  type="button"
                  onClick={() => remove(index)}
                >
                  削除
                </Button>
              </fieldset>
            ))}
          </div>
          <div className="flex flex-wrap gap-2">
            <Button
              variant="outline"
              type="button"
              disabled={macros.length >= MAX_MATH_MACROS}
              onClick={() => add()}
            >
              マクロを追加
            </Button>
            <Button
              type="submit"
              disabled={saving || validationProblem !== null}
            >
              {saving ? "保存しています…" : "保存"}
            </Button>
          </div>
          {validationProblem && (
            <ProblemAlert>{validationProblem}</ProblemAlert>
          )}
          {message &&
            (failed ? (
              <ProblemAlert>{message}</ProblemAlert>
            ) : (
              <StatusMessage>{message}</StatusMessage>
            ))}
        </form>
      )}
    </section>
  );
}

function saveFailureMessage(error: unknown): string {
  if (error instanceof ApiError && error.problem.code === "conflict") {
    return "別の画面で数式マクロが更新されています。この画面の内容を控えてから再読み込みし、最新の設定へ反映してください。";
  }
  if (error instanceof ApiError && error.problem.code === "validation_failed") {
    return "入力内容が保存条件を満たしていません。件数、全体の大きさ、コマンド名、引数の参照を確認してください。";
  }
  return "数式マクロを保存できませんでした。通信状態を確認して、もう一度お試しください。";
}
