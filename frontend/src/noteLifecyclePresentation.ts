import { ApiError } from "./api";

export function noteDeletionProblem(error: unknown): string {
  if (error instanceof ApiError) {
    switch (error.status) {
      case 409:
        return "別の操作でノートが更新されました。取り消して画面を再読み込みしてから、もう一度削除してください。";
      case 403:
      case 404:
        return "このノートを削除する権限を確認できませんでした。取り消して一覧から開き直してください。";
      default:
        return "ノートを削除できませんでした。時間を置いてから、もう一度実行してください。";
    }
  }
  return "サーバーと通信できませんでした。接続を確認してから、もう一度実行してください。";
}

export function noteRestorationProblem(error: unknown): string {
  if (error instanceof ApiError) {
    switch (error.status) {
      case 409:
        return "別の操作でノートが更新されました。取り消して一覧を再読み込みしてから、もう一度復元してください。";
      case 410:
        return "このノートの復元期限を過ぎています。取り消して一覧を再読み込みしてください。";
      case 403:
      case 404:
        return "このノートを復元する権限を確認できませんでした。取り消して一覧を再読み込みしてください。";
      default:
        return "ノートを復元できませんでした。時間を置いてから、もう一度実行してください。";
    }
  }
  return "サーバーと通信できませんでした。接続を確認してから、もう一度実行してください。";
}
