import { FormEvent, useCallback, useMemo } from "react";

import { ProblemAlert, StatusMessage } from "@/components/feedback";
import { PageHeader } from "@/components/PageHeader";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";

import { ApplicationConfig, listNotes, NoteListEntry } from "../api";
import { formatDateTime } from "../formatting";
import {
  NoteListQuery,
  parseNoteListQuery,
  selectNoteListPage,
} from "../noteListState";
import { deletedNotesPath, externalPath, listPath, notePath } from "../paths";
import { useApiResource } from "../useApiResource";

export function NoteListPage({ config }: { config: ApplicationConfig }) {
  const query = useMemo(
    () => parseNoteListQuery(config.search),
    [config.search],
  );
  const load = useCallback(
    (signal: AbortSignal) => listNotes(config.apiBase, signal),
    [config.apiBase],
  );
  const resource = useApiResource(load);
  const notes = resource.status === "ready" ? resource.value : null;
  const failed = resource.status === "failed";
  const page = notes === null ? null : selectNoteListPage(notes, query);
  const notice = new URLSearchParams(config.search).get("notice");
  return (
    <section className="grid gap-6" aria-labelledby="note-index-heading">
      <PageHeader
        eyebrow="Library"
        title="ノート"
        titleId="note-index-heading"
        description="記録した知識を、更新日やタグから見つけられます。"
      >
        <Button variant="outline" asChild>
          <a href={deletedNotesPath(config)}>削除済みノート</a>
        </Button>
      </PageHeader>
      {notice === "note-deleted" && (
        <StatusMessage>
          ノートを削除しました。削除後30日以内であれば復元できます。
        </StatusMessage>
      )}
      {notice === "note-restored" && (
        <StatusMessage>ノートを復元しました。</StatusMessage>
      )}
      <NoteListFilters config={config} query={query} />
      {failed ? (
        <ProblemAlert>ノート一覧を読み込めませんでした。</ProblemAlert>
      ) : notes === null ? (
        <StatusMessage>ノート一覧を読み込んでいます。</StatusMessage>
      ) : page?.total === 0 ? (
        <StatusMessage>
          {notes.length === 0
            ? "閲覧できるノートはありません。"
            : "条件に一致するノートはありません。"}
        </StatusMessage>
      ) : (
        <>
          <p className="m-0 text-sm text-muted-foreground" role="status">
            {page?.total}件のノート
          </p>
          <ul className="m-0 grid list-none gap-3 p-0">
            {page?.notes.map((note) => (
              <li
                key={note.note_id}
                className="rounded-md border bg-card px-5 py-4 shadow-xs transition hover:border-input hover:shadow-md"
              >
                <a
                  href={notePath(config, note.note_id)}
                  className="font-bold text-foreground no-underline hover:text-primary"
                >
                  {note.title}
                </a>
                <dl className="my-2 flex flex-wrap gap-x-5 gap-y-2 text-sm">
                  <div className="flex gap-1">
                    <dt className="m-0 font-semibold text-muted-foreground">
                      更新
                    </dt>
                    <dd className="m-0">
                      <time
                        dateTime={new Date(note.updated_at_ms).toISOString()}
                      >
                        {formatDateTime(note.updated_at_ms)}
                      </time>
                    </dd>
                  </div>
                  <div className="flex gap-1">
                    <dt className="m-0 font-semibold text-muted-foreground">
                      アクセス
                    </dt>
                    <dd className="m-0">{accessLabel(note.access)}</dd>
                  </div>
                  <div className="flex gap-1">
                    <dt className="m-0 font-semibold text-muted-foreground">
                      作成経路
                    </dt>
                    <dd className="m-0">
                      {creationSourceLabel(note.created_via)}
                    </dd>
                  </div>
                  <div className="flex gap-1">
                    <dt className="m-0 font-semibold text-muted-foreground">
                      人手確認
                    </dt>
                    <dd className="m-0">
                      {reviewStatusLabel(note.review_status)}
                    </dd>
                  </div>
                </dl>
                {note.tags.length > 0 && (
                  <ul
                    className="m-0 flex list-none flex-wrap gap-2 p-0"
                    aria-label="ノートのタグ"
                  >
                    {note.tags.map((tag) => (
                      <li key={tag}>
                        <Badge variant="secondary">{tag}</Badge>
                      </li>
                    ))}
                  </ul>
                )}
              </li>
            ))}
          </ul>
          {page && page.pageCount > 1 && (
            <nav
              className="flex items-center justify-center gap-4"
              aria-label="ノート一覧のページ"
            >
              {page.page > 1 && (
                <Button variant="outline" size="sm" asChild>
                  <a href={listPath(config, page.page - 1)}>前へ</a>
                </Button>
              )}
              <span className="text-sm text-muted-foreground">
                {page.page} / {page.pageCount}
              </span>
              {page.page < page.pageCount && (
                <Button variant="outline" size="sm" asChild>
                  <a href={listPath(config, page.page + 1)}>次へ</a>
                </Button>
              )}
            </nav>
          )}
        </>
      )}
    </section>
  );
}

function creationSourceLabel(source: NoteListEntry["created_via"]): string {
  switch (source) {
    case "web":
      return "Web UI";
    case "rest":
      return "REST API";
    case "mcp":
      return "MCP";
    case "unknown":
      return "不明";
  }
}

function reviewStatusLabel(status: NoteListEntry["review_status"]): string {
  switch (status) {
    case "reviewed":
      return "確認済み";
    case "pending":
      return "確認待ち";
    case "unknown":
      return "不明";
  }
}

function NoteListFilters({
  config,
  query,
}: {
  config: ApplicationConfig;
  query: NoteListQuery;
}) {
  function resetPage(event: FormEvent<HTMLFormElement>) {
    const page = event.currentTarget.elements.namedItem("page");
    if (page instanceof HTMLInputElement) page.value = "1";
  }
  return (
    <form
      className="grid items-stretch gap-4 rounded-md border bg-card p-4 shadow-xs min-[60rem]:flex min-[60rem]:flex-wrap min-[60rem]:items-end"
      action={externalPath(config.basePath, "/")}
      method="get"
      onSubmit={resetPage}
    >
      <label className="grid gap-1 text-sm font-semibold">
        タグ
        <Input
          className="min-[60rem]:min-w-[min(16rem,78vw)]"
          name="tag"
          type="text"
          defaultValue={query.tags.join(", ")}
          placeholder="research, rust"
        />
      </label>
      <label className="grid gap-1 text-sm font-semibold">
        この日以降に更新
        <Input
          className="min-[60rem]:min-w-[min(16rem,78vw)]"
          name="updated_after"
          type="date"
          defaultValue={query.updatedAfter}
        />
      </label>
      <label className="grid gap-1 text-sm font-semibold">
        人手確認
        <select name="review_status" defaultValue={query.reviewStatus}>
          <option value="">すべて</option>
          <option value="pending">確認待ち</option>
          <option value="reviewed">確認済み</option>
        </select>
      </label>
      <input name="page" type="hidden" value="1" readOnly />
      <Button variant="outline" type="submit">
        絞り込む
      </Button>
      {(query.tags.length > 0 || query.updatedAfter || query.reviewStatus) && (
        <Button variant="ghost" asChild>
          <a href={externalPath(config.basePath, "/")}>条件を解除</a>
        </Button>
      )}
    </form>
  );
}

function accessLabel(access: NoteListEntry["access"]): string {
  switch (access) {
    case "read":
      return "閲覧";
    case "edit":
      return "編集";
    case "manage":
      return "所有";
  }
}
