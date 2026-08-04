import { FormEvent, useCallback, useMemo } from "react";

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
    <section
      className="note-index page-section"
      aria-labelledby="note-index-heading"
    >
      <div className="page-heading">
        <div>
          <p className="page-eyebrow">Library</p>
          <h1 id="note-index-heading">ノート</h1>
          <p className="page-description">
            記録した知識を、更新日やタグから見つけられます。
          </p>
        </div>
        <a className="button button-secondary" href={deletedNotesPath(config)}>
          削除済みノート
        </a>
      </div>
      {notice === "note-deleted" && (
        <p className="notice" role="status">
          ノートを削除しました。削除後30日以内であれば復元できます。
        </p>
      )}
      {notice === "note-restored" && (
        <p className="notice" role="status">
          ノートを復元しました。
        </p>
      )}
      <NoteListFilters config={config} query={query} />
      {failed ? (
        <p className="problem-inline" role="alert">
          ノート一覧を読み込めませんでした。
        </p>
      ) : notes === null ? (
        <p className="state-message" role="status">
          ノート一覧を読み込んでいます。
        </p>
      ) : page?.total === 0 ? (
        <p className="state-message">
          {notes.length === 0
            ? "閲覧できるノートはありません。"
            : "条件に一致するノートはありません。"}
        </p>
      ) : (
        <>
          <p className="list-result-count" role="status">
            {page?.total}件のノート
          </p>
          <ul className="note-list">
            {page?.notes.map((note) => (
              <li key={note.note_id}>
                <a href={notePath(config, note.note_id)}>{note.title}</a>
                <dl>
                  <div>
                    <dt>更新</dt>
                    <dd>
                      <time
                        dateTime={new Date(note.updated_at_ms).toISOString()}
                      >
                        {formatDateTime(note.updated_at_ms)}
                      </time>
                    </dd>
                  </div>
                  <div>
                    <dt>アクセス</dt>
                    <dd>{accessLabel(note.access)}</dd>
                  </div>
                </dl>
                {note.tags.length > 0 && (
                  <ul className="tag-list" aria-label="ノートのタグ">
                    {note.tags.map((tag) => (
                      <li key={tag}>{tag}</li>
                    ))}
                  </ul>
                )}
              </li>
            ))}
          </ul>
          {page && page.pageCount > 1 && (
            <nav className="pagination" aria-label="ノート一覧のページ">
              {page.page > 1 && (
                <a href={listPath(config, page.page - 1)}>前へ</a>
              )}
              <span>
                {page.page} / {page.pageCount}
              </span>
              {page.page < page.pageCount && (
                <a href={listPath(config, page.page + 1)}>次へ</a>
              )}
            </nav>
          )}
        </>
      )}
    </section>
  );
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
      className="note-list-filters"
      action={externalPath(config.basePath, "/")}
      method="get"
      onSubmit={resetPage}
    >
      <label>
        タグ
        <input
          name="tag"
          type="text"
          defaultValue={query.tags.join(", ")}
          placeholder="research, rust"
        />
      </label>
      <label>
        この日以降に更新
        <input
          name="updated_after"
          type="date"
          defaultValue={query.updatedAfter}
        />
      </label>
      <input name="page" type="hidden" value="1" readOnly />
      <button type="submit">絞り込む</button>
      {(query.tags.length > 0 || query.updatedAfter) && (
        <a href={externalPath(config.basePath, "/")}>条件を解除</a>
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
