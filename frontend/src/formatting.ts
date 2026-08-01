/** 画面へ出す値の書式。同じ種類の値が画面ごとに違う見え方にならないよう、ここへ集めます。 */

/** 保存している時刻を、閲覧者の地域の書式で日付と時刻に直します。 */
export function formatDateTime(timestamp: number): string {
  return new Intl.DateTimeFormat(undefined, {
    dateStyle: "medium",
    timeStyle: "short",
  }).format(new Date(timestamp));
}
