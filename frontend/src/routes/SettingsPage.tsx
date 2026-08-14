import { PageHeader } from "@/components/PageHeader";

import { ApplicationConfig } from "../api";
import { externalPath } from "../paths";

export function SettingsPage({ config }: { config: ApplicationConfig }) {
  return (
    <section className="grid gap-6">
      <PageHeader
        eyebrow="Settings"
        title="設定"
        description="ノートの表示方法、外部のMCPクライアントへ許可できる操作、変更のWebhook通知を管理します。"
      />
      <div className="grid gap-4 sm:grid-cols-2">
        <SettingsCard
          href={externalPath(config.basePath, "/settings/math-macros")}
          title="数式マクロ"
          description="所有するノートで使うMathJaxコマンドを定義します。"
        />
        <SettingsCard
          href={externalPath(config.basePath, "/settings/mcp-access")}
          title="MCPのアクセス制御"
          description="すべてのMCPクライアントへ許可できる操作の上限を設定します。"
        />
        <SettingsCard
          href={externalPath(config.basePath, "/settings/webhooks")}
          title="Webhook通知"
          description="ノートと文献情報の変更を外部のURLへ通知します。"
        />
      </div>
    </section>
  );
}

function SettingsCard({
  href,
  title,
  description,
}: {
  href: string;
  title: string;
  description: string;
}) {
  return (
    <a
      href={href}
      className="grid gap-1 rounded-lg border bg-card p-4 text-card-foreground no-underline transition-colors hover:border-input hover:bg-muted"
    >
      <strong className="text-foreground">{title}</strong>
      <span className="text-sm text-muted-foreground">{description}</span>
    </a>
  );
}
