const DAY_MS = 24 * 60 * 60 * 1_000;

export interface NoteRetentionStatus {
  expired: boolean;
  label: string;
}

export function noteRetentionStatus(
  purgeAtMs: number,
  nowMs = Date.now(),
): NoteRetentionStatus {
  if (purgeAtMs < nowMs) {
    return { expired: true, label: "復元期限を過ぎています。" };
  }
  const remainingDays = Math.ceil((purgeAtMs - nowMs) / DAY_MS);
  return {
    expired: false,
    label:
      remainingDays === 0
        ? "本日まで復元できます。"
        : `復元期限まで${remainingDays}日です。`,
  };
}
