import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

import plugin from "@adocweave/textlint-plugin-asciidoc";
import { TextlintKernel } from "@textlint/kernel";
import commentsFilter from "textlint-filter-rule-comments";
import technicalWriting from "textlint-rule-preset-ja-technical-writing";

const repositoryRoot = fileURLToPath(new URL("../../", import.meta.url));
const selectedRuleIds = [
  "no-mix-dearu-desumasu",
  "no-double-negative-ja",
  "no-dropping-the-ra",
  "no-nfd",
  "no-hankaku-kana",
  "no-invalid-control-character",
  "no-unmatched-pair",
  "no-zero-width-spaces"
];

const rules = selectedRuleIds.map((ruleId) => {
  const rule = technicalWriting.rules[ruleId];
  if (rule === null || (typeof rule !== "function" && typeof rule !== "object")) {
    throw new Error(`日本語技術文書規則が見つかりません: ${ruleId}`);
  }
  return {
    ruleId,
    rule,
    options:
      ruleId === "no-mix-dearu-desumasu"
        ? { preferInHeader: "", preferInBody: "ですます", preferInList: "ですます", strict: false }
        : structuredClone(technicalWriting.rulesConfig[ruleId])
  };
});

const paths = process.argv.slice(2);
if (paths.length === 0) {
  throw new Error("文章校正の対象となるAsciiDoc文書がありません。");
}

const kernel = new TextlintKernel();
let violations = 0;
for (const path of paths) {
  const absolute = `${repositoryRoot}${path}`;
  const source = readFileSync(absolute, "utf8");
  const before = createHash("sha256").update(source).digest("hex");
  const result = await kernel.lintText(source, {
    ext: ".adoc",
    filePath: absolute,
    plugins: [{ pluginId: "adocweave", plugin }],
    rules,
    filterRules: [{ ruleId: "comments", rule: commentsFilter }]
  });
  const after = createHash("sha256").update(readFileSync(absolute)).digest("hex");
  if (before !== after) {
    throw new Error(`文章校正が文書を書き換えました: ${path}`);
  }
  for (const message of result.messages) {
    violations += 1;
    console.error(`${path}:${message.line}:${message.column}: ${message.message} (${message.ruleId})`);
  }
}

if (violations > 0) {
  process.exitCode = 1;
}
