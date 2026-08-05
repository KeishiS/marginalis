import { ApplicationConfig } from "../api";
import { externalPath } from "../paths";

export function SettingsPage({ config }: { config: ApplicationConfig }) {
  return (
    <section className="page-section settings-index">
      <div className="page-heading">
        <div>
          <p className="page-eyebrow">Settings</p>
          <h1>設定</h1>
          <p className="page-description">
            ノートの表示方法と、外部のMCPクライアントへ許可できる操作を管理します。
          </p>
        </div>
      </div>
      <div className="settings-card-list">
        <a
          className="surface settings-card"
          href={externalPath(config.basePath, "/settings/math-macros")}
        >
          <strong>数式マクロ</strong>
          <span>所有するノートで使うMathJaxコマンドを定義します。</span>
        </a>
        <a
          className="surface settings-card"
          href={externalPath(config.basePath, "/settings/mcp-access")}
        >
          <strong>MCPのアクセス制御</strong>
          <span>
            すべてのMCPクライアントへ許可できる操作の上限を設定します。
          </span>
        </a>
      </div>
    </section>
  );
}
