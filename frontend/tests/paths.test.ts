import { describe, expect, it } from "vitest";

import {
  accessPath,
  canonicalSearch,
  editPath,
  externalPath,
  graphPath,
  listPath,
  notePath,
} from "../src/paths";

const root = { basePath: "/", search: "" };
const subpath = { basePath: "/marginalis", search: "" };

describe("画面URLの組み立て", () => {
  it("サブパス配置ではprefixを付ける", () => {
    expect(externalPath("/marginalis", "/notes/1")).toBe("/marginalis/notes/1");
    expect(externalPath("/", "/notes/1")).toBe("/notes/1");
    expect(notePath(subpath, "1")).toBe("/marginalis/notes/1");
  });

  /// 一覧の条件は、どの画面から組み立てても同じURLになる。
  it("絞り込み条件を正規化してから連結する", () => {
    const messy = { basePath: "/", search: "?tag=b&tag=b&tag=a&page=0" };
    const expected = canonicalSearch(messy.search);
    expect(notePath(messy, "1")).toBe(`/notes/1${expected}`);
    expect(editPath(messy, "1")).toBe(`/notes/1/edit${expected}`);
    expect(accessPath(messy, "1")).toBe(`/notes/1/access${expected}`);
  });

  /// 以前は編集画面だけが生の値を連結しており、閲覧画面と異なるURLになりえた。
  it("重複したタグを含む条件でも閲覧と編集で同じ問い合わせになる", () => {
    const messy = { basePath: "/", search: "?tag=research&tag=research" };
    const view = notePath(messy, "1").split("?")[1] ?? "";
    const edit = editPath(messy, "1").split("?")[1] ?? "";
    expect(edit).toBe(view);
  });

  it("一覧はページ位置を含められる", () => {
    expect(listPath(root)).toBe("/");
    expect(listPath(root, 2)).toContain("page=2");
  });

  /// 一覧の絞り込みはタグと更新日を対象とし、図の範囲とは別の条件である。
  it("関係の図は起点と階層を持ち、一覧の条件を引き継がない", () => {
    const messy = { basePath: "/marginalis", search: "?tag=research&page=3" };
    expect(graphPath(messy)).toBe("/marginalis/graph");
    expect(graphPath(messy, { noteId: "note-1", depth: 2 })).toBe(
      "/marginalis/graph?origin=note-1&depth=2",
    );
  });
});
