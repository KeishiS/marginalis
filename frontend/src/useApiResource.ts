import { useEffect, useState } from "react";

/** 読み込み中の資源の状態。 */
export type ApiResource<T> =
  | { status: "loading" }
  | { status: "ready"; value: T }
  | { status: "failed"; error: unknown };

type Loader<T> = (signal: AbortSignal) => Promise<T>;

/**
 * REST APIから1件の資源を読み込む。
 *
 * 画面を離れた時点で進行中の要求を中止し、結果を捨てる。画面ごとに取消の方法が
 * 分かれると、取消し忘れに気づけないため、読み込みはこのhookへ集約する。
 *
 * `load`は`useCallback`で包んで渡す。依存の指定漏れを`react-hooks`の検査が
 * 呼び出し側で確認できるようにするため。
 *
 * 読み込み中かどうかは、結果に紐づく`load`と現在の`load`を比べて導出する。
 * effectの中で同期的に状態を戻すと余分な再描画を招くため、その方法は使わない。
 */
export function useApiResource<T>(load: Loader<T>): ApiResource<T> {
  const [completed, setCompleted] = useState<{
    load: Loader<T>;
    resource: ApiResource<T>;
  } | null>(null);

  useEffect(() => {
    const controller = new AbortController();
    load(controller.signal)
      .then((value) => {
        if (!controller.signal.aborted) {
          setCompleted({ load, resource: { status: "ready", value } });
        }
      })
      .catch((error: unknown) => {
        if (!controller.signal.aborted) {
          setCompleted({ load, resource: { status: "failed", error } });
        }
      });
    return () => controller.abort();
  }, [load]);

  return completed?.load === load ? completed.resource : { status: "loading" };
}
