import { FormEvent, useEffect, useState } from "react";

import {
  ApplicationConfig,
  WebhookEventKind,
  WebhookSubscription,
  createWebhookSubscription,
  deleteWebhookSubscription,
  discardWebhookDelivery,
  listWebhookSubscriptions,
  regenerateWebhookSecret,
  retryWebhookDelivery,
  verifyWebhookSubscription,
} from "../api";
import { ProblemAlert, StatusMessage } from "@/components/feedback";
import { PageHeader } from "@/components/PageHeader";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";

import { ConfirmationDialog } from "../ConfirmationDialog";
import { externalPath } from "../paths";

const EVENT_KIND_DESCRIPTIONS: Record<WebhookEventKind, string> = {
  "note.created": "ノートの作成",
  "note.updated": "ノート本文の更新",
  "note.deleted": "ノートの削除",
  "note.restored": "ノートの復元",
  "bibliography_item.created": "書誌情報の追加",
  "bibliography_item.updated": "書誌情報の更新",
  "bibliography_item.deleted": "書誌情報の削除",
};

const EVENT_KINDS = Object.keys(EVENT_KIND_DESCRIPTIONS) as WebhookEventKind[];

const STATE_LABELS: Record<WebhookSubscription["state"], string> = {
  pending_challenge: "検証待ち",
  active: "有効",
  disabled: "停止中",
};

const FAILURE_LABELS: Record<string, string> = {
  non_success_status: "受信側が成功以外の応答を返しました",
  connect_failed: "送信先へ接続できませんでした",
  timed_out: "送信が時間内に完了しませんでした",
  destination_rejected: "送信先URLが許可されない宛先でした",
};

const DISABLED_REASON_LABELS: Record<string, string> = {
  delivery_exhausted: "配送の再試行が上限に達したため停止しています",
  destination_rejected: "送信先が拒否されたため停止しています",
  owner_disabled: "所有者の操作で停止しています",
};

export function WebhookSettingsPage({ config }: { config: ApplicationConfig }) {
  const [subscriptions, setSubscriptions] = useState<
    WebhookSubscription[] | null
  >(null);
  const [url, setUrl] = useState("");
  const [selectedKinds, setSelectedKinds] = useState<WebhookEventKind[]>([]);
  const [creating, setCreating] = useState(false);
  const [busySubscription, setBusySubscription] = useState<string | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<WebhookSubscription | null>(
    null,
  );
  // 表示中のsecret。応答で受け取ったこの1回しか確認できない。
  const [revealedSecret, setRevealedSecret] = useState<{
    subscriptionId: string;
    secret: string;
  } | null>(null);
  const [message, setMessage] = useState("");
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    const controller = new AbortController();
    listWebhookSubscriptions(config.apiBase, controller.signal)
      .then((value) => {
        if (!controller.signal.aborted) {
          setSubscriptions(value);
        }
      })
      .catch(() => {
        if (!controller.signal.aborted) {
          setFailed(true);
          setMessage("Webhookの設定を読み込めませんでした。");
        }
      });
    return () => controller.abort();
  }, [config.apiBase]);

  async function reload() {
    setSubscriptions(await listWebhookSubscriptions(config.apiBase));
  }

  function toggleKind(kind: WebhookEventKind, checked: boolean) {
    setSelectedKinds((current) =>
      checked ? [...current, kind] : current.filter((value) => value !== kind),
    );
    setMessage("");
  }

  async function create(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (creating) return;
    setCreating(true);
    setMessage("");
    try {
      const ordered = EVENT_KINDS.filter((kind) =>
        selectedKinds.includes(kind),
      );
      const created = await createWebhookSubscription(config.apiBase, {
        url: url.trim(),
        event_kinds: ordered,
      });
      setRevealedSecret({
        subscriptionId: created.subscription.subscription_id,
        secret: created.secret,
      });
      setUrl("");
      setSelectedKinds([]);
      setFailed(false);
      setMessage(
        "Webhookを登録しました。下のsecretを控えたうえで、送信先の確認を実行してください。",
      );
      await reload();
    } catch {
      setFailed(true);
      setMessage(
        "Webhookを登録できませんでした。送信先は公開されたHTTPS(port 443)のURLだけを使えます。",
      );
    } finally {
      setCreating(false);
    }
  }

  async function verify(subscription: WebhookSubscription) {
    if (busySubscription !== null) return;
    setBusySubscription(subscription.subscription_id);
    setMessage("");
    try {
      const outcome = await verifyWebhookSubscription(
        config.apiBase,
        subscription.subscription_id,
      );
      if (outcome.verified) {
        setFailed(false);
        setMessage("送信先を確認し、Webhookを有効にしました。");
      } else {
        setFailed(true);
        setMessage(
          `送信先を確認できませんでした。${outcome.failure ? (FAILURE_LABELS[outcome.failure] ?? "") : ""}受信側が署名付きchallengeへ応答することを確かめてください。`,
        );
      }
      await reload();
    } catch {
      setFailed(true);
      setMessage("送信先の確認を実行できませんでした。");
    } finally {
      setBusySubscription(null);
    }
  }

  async function regenerate(subscription: WebhookSubscription) {
    if (busySubscription !== null) return;
    setBusySubscription(subscription.subscription_id);
    setMessage("");
    try {
      const value = await regenerateWebhookSecret(
        config.apiBase,
        subscription.subscription_id,
      );
      setRevealedSecret({
        subscriptionId: subscription.subscription_id,
        secret: value.secret,
      });
      setFailed(false);
      setMessage(
        "secretを再生成しました。下の値を控えて、受信側の検証設定を更新してください。",
      );
      await reload();
    } catch {
      setFailed(true);
      setMessage("secretを再生成できませんでした。");
    } finally {
      setBusySubscription(null);
    }
  }

  async function retry(subscription: WebhookSubscription) {
    if (busySubscription !== null) return;
    setBusySubscription(subscription.subscription_id);
    setMessage("");
    try {
      await retryWebhookDelivery(config.apiBase, subscription.subscription_id);
      setFailed(false);
      setMessage("最も古い配送を直ちに再試行するよう予約しました。");
      await reload();
    } catch {
      setFailed(true);
      setMessage("配送の再試行を予約できませんでした。");
    } finally {
      setBusySubscription(null);
    }
  }

  async function discard(subscription: WebhookSubscription) {
    if (busySubscription !== null) return;
    setBusySubscription(subscription.subscription_id);
    setMessage("");
    try {
      await discardWebhookDelivery(
        config.apiBase,
        subscription.subscription_id,
      );
      setFailed(false);
      setMessage("最も古い配送を破棄しました。後続の配送が順に進みます。");
      await reload();
    } catch {
      setFailed(true);
      setMessage("配送を破棄できませんでした。");
    } finally {
      setBusySubscription(null);
    }
  }

  async function remove() {
    if (deleteTarget === null || busySubscription !== null) return;
    const subscriptionId = deleteTarget.subscription_id;
    setBusySubscription(subscriptionId);
    try {
      await deleteWebhookSubscription(config.apiBase, subscriptionId);
      if (revealedSecret?.subscriptionId === subscriptionId) {
        setRevealedSecret(null);
      }
      setFailed(false);
      setMessage("Webhookを削除しました。");
      setDeleteTarget(null);
      await reload();
    } catch {
      setFailed(true);
      setMessage("Webhookを削除できませんでした。");
    } finally {
      setBusySubscription(null);
    }
  }

  return (
    <section className="grid gap-6">
      <PageHeader
        eyebrow="Settings"
        title="Webhook通知"
        description="ノートと書誌情報の変更を、指定したURLへHTTP POSTで通知します。通知の本文には変更の種別と対象IDだけが含まれ、ノート本文は含まれません。"
      >
        <Button variant="outline" asChild>
          <a href={externalPath(config.basePath, "/settings")}>設定へ戻る</a>
        </Button>
      </PageHeader>
      {subscriptions === null ? (
        !message ? (
          <StatusMessage>Webhookの設定を読み込んでいます。</StatusMessage>
        ) : (
          <ProblemAlert>{message}</ProblemAlert>
        )
      ) : (
        <div className="grid gap-5">
          {message ? (
            failed ? (
              <ProblemAlert>{message}</ProblemAlert>
            ) : (
              <StatusMessage>{message}</StatusMessage>
            )
          ) : null}
          {revealedSecret !== null ? (
            <div className="grid gap-2 rounded-md border bg-card p-5 shadow-xs">
              <strong>署名検証用のsecret</strong>
              <p className="m-0 text-sm text-muted-foreground">
                この値は今だけ表示され、あとから確認できません。受信側で署名の検証に使う値として安全な場所へ保存してください。
              </p>
              <code
                data-slot="webhook-secret"
                className="rounded-sm bg-muted p-3 [overflow-wrap:anywhere]"
              >
                {revealedSecret.secret}
              </code>
              <div>
                <Button
                  variant="outline"
                  type="button"
                  onClick={() => setRevealedSecret(null)}
                >
                  secretの表示を閉じる
                </Button>
              </div>
            </div>
          ) : null}
          <form
            className="grid gap-4 rounded-md border bg-card p-5 shadow-xs"
            onSubmit={(event) => void create(event)}
          >
            <fieldset className="m-0 grid gap-3 border-0 p-0">
              <legend className="mb-1 font-bold">Webhookを登録</legend>
              <label className="grid gap-1">
                <span className="text-sm font-semibold">送信先URL</span>
                <Input
                  type="url"
                  required
                  placeholder="https://receiver.example.com/hooks/marginalis"
                  value={url}
                  onChange={(event) => {
                    setUrl(event.target.value);
                    setMessage("");
                  }}
                />
                <small className="text-muted-foreground">
                  公開されたHTTPS(port
                  443)のURLだけを登録できます。登録後に送信先の確認を実行すると通知が始まります。
                </small>
              </label>
              <div className="grid gap-2">
                <span className="text-sm font-semibold">通知するevent</span>
                {EVENT_KINDS.map((kind) => (
                  <label
                    key={kind}
                    className="grid grid-cols-[auto_1fr] items-start gap-3 rounded-sm bg-muted p-3"
                  >
                    <input
                      type="checkbox"
                      checked={selectedKinds.includes(kind)}
                      onChange={(event) =>
                        toggleKind(kind, event.target.checked)
                      }
                    />
                    <span className="grid gap-1">
                      <code>{kind}</code>
                      <small className="text-muted-foreground">
                        {EVENT_KIND_DESCRIPTIONS[kind]}
                      </small>
                    </span>
                  </label>
                ))}
              </div>
            </fieldset>
            <Button disabled={creating || selectedKinds.length === 0}>
              {creating ? "登録しています…" : "Webhookを登録"}
            </Button>
          </form>
          <section aria-labelledby="webhook-subscriptions-heading">
            <div className="mb-4">
              <h2
                id="webhook-subscriptions-heading"
                className="m-0 text-xl font-bold"
              >
                登録済みのWebhook
              </h2>
              <p className="m-0 text-muted-foreground">
                送信先ごとの状態と配送の滞留を確認し、検証・再試行・削除ができます。
              </p>
            </div>
            {subscriptions.length === 0 ? (
              <StatusMessage>登録済みのWebhookはありません。</StatusMessage>
            ) : (
              <div className="grid gap-5">
                {subscriptions.map((subscription) => (
                  <article
                    key={subscription.subscription_id}
                    className="grid min-w-0 gap-4 rounded-md border bg-card p-5 shadow-xs"
                  >
                    <div className="flex items-start justify-between gap-3">
                      <code className="min-w-0 text-sm [overflow-wrap:anywhere]">
                        {subscription.url}
                      </code>
                      <Badge
                        variant={
                          subscription.state === "active"
                            ? "secondary"
                            : subscription.state === "disabled"
                              ? "destructive"
                              : "outline"
                        }
                      >
                        {STATE_LABELS[subscription.state]}
                      </Badge>
                    </div>
                    <div className="flex flex-wrap gap-1">
                      {subscription.event_kinds.map((kind) => (
                        <Badge key={kind} variant="outline">
                          {kind}
                        </Badge>
                      ))}
                    </div>
                    {subscription.state === "disabled" &&
                    subscription.disabled_reason !== null ? (
                      <ProblemAlert>
                        {DISABLED_REASON_LABELS[subscription.disabled_reason] ??
                          "停止しています"}
                        。失敗した配送を再試行または破棄すると再開します。
                      </ProblemAlert>
                    ) : null}
                    <dl className="m-0 grid gap-3 sm:grid-cols-2">
                      <div className="grid min-w-0 gap-1 rounded-sm bg-muted p-3">
                        <dt className="text-sm font-semibold text-muted-foreground">
                          配送待ち
                        </dt>
                        <dd className="m-0">
                          {subscription.pending_count === 0
                            ? "なし"
                            : `${subscription.pending_count}件`}
                        </dd>
                      </div>
                      <div className="grid min-w-0 gap-1 rounded-sm bg-muted p-3">
                        <dt className="text-sm font-semibold text-muted-foreground">
                          直近の配送
                        </dt>
                        <dd className="m-0">
                          {subscription.last_failure !== null
                            ? (FAILURE_LABELS[subscription.last_failure] ??
                              subscription.last_failure)
                            : subscription.last_attempted_at_ms !== null
                              ? `成功 (${formatTimestamp(subscription.last_attempted_at_ms)})`
                              : "まだ配送していません"}
                        </dd>
                      </div>
                    </dl>
                    <div className="flex flex-wrap gap-2">
                      {subscription.state !== "active" ? (
                        <Button
                          type="button"
                          disabled={busySubscription !== null}
                          onClick={() => void verify(subscription)}
                        >
                          送信先を確認して有効化
                        </Button>
                      ) : null}
                      {subscription.last_failure !== null ? (
                        <>
                          <Button
                            type="button"
                            disabled={busySubscription !== null}
                            onClick={() => void retry(subscription)}
                          >
                            失敗した配送を再試行
                          </Button>
                          <Button
                            variant="outline"
                            type="button"
                            disabled={busySubscription !== null}
                            onClick={() => void discard(subscription)}
                          >
                            失敗した配送を破棄
                          </Button>
                        </>
                      ) : null}
                      <Button
                        variant="outline"
                        type="button"
                        disabled={busySubscription !== null}
                        onClick={() => void regenerate(subscription)}
                      >
                        secretを再生成
                      </Button>
                      <Button
                        variant="destructive"
                        type="button"
                        disabled={busySubscription !== null}
                        onClick={() => setDeleteTarget(subscription)}
                      >
                        削除
                      </Button>
                    </div>
                  </article>
                ))}
              </div>
            )}
          </section>
        </div>
      )}
      {deleteTarget !== null ? (
        <ConfirmationDialog
          eyebrow="Webhook"
          heading="Webhookを削除しますか？"
          description="この送信先への通知を直ちに停止し、配送待ちのeventも破棄します。この操作は取り消せません。"
          confirmLabel="削除する"
          busyLabel="削除しています…"
          destructive
          busy={busySubscription !== null}
          problem={failed ? message : null}
          onCancel={() => setDeleteTarget(null)}
          onConfirm={() => void remove()}
        />
      ) : null}
    </section>
  );
}

function formatTimestamp(value: number): string {
  return new Intl.DateTimeFormat("ja-JP", {
    dateStyle: "medium",
    timeStyle: "short",
  }).format(new Date(value));
}
