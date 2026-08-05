import assert from "node:assert/strict";
import test from "node:test";

import plugin from "@adocweave/textlint-plugin-asciidoc";
import { TextlintKernel } from "@textlint/kernel";

function halfwidthKanaRule(context) {
  const { RuleError, Syntax, report } = context;
  return {
    [Syntax.Str](node) {
      if (node.value.includes("ｶﾅ")) {
        report(node, new RuleError("自然文に半角カナがあります。"));
      }
    }
  };
}

async function lint(source) {
  const kernel = new TextlintKernel();
  return kernel.lintText(source, {
    ext: ".adoc",
    filePath: "document.adoc",
    plugins: [{ pluginId: "adocweave", plugin }],
    rules: [
      {
        ruleId: "no-hankaku-kana",
        rule: halfwidthKanaRule
      }
    ]
  });
}

test("自然文を校正し、source blockとinline codeを対象外にする", async () => {
  const prose = await lint("= 文書\n\n半角ｶﾅです。\n");
  assert.equal(prose.messages.length, 1);

  const code = await lint(
    "= 文書\n\n[source,text]\n----\n半角ｶﾅ\n----\n\n+半角ｶﾅ+は識別子の例です。\n"
  );
  assert.equal(code.messages.length, 0);
});
