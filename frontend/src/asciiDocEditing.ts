export const ASCII_DOC_COMMANDS = [
  "title",
  "section",
  "list",
  "link",
  "code-block",
  "inline-math",
  "block-math",
  "note-reference",
] as const;

export type AsciiDocCommand = (typeof ASCII_DOC_COMMANDS)[number];

export interface TextEdit {
  from: number;
  to: number;
  insert: string;
  selection: {
    anchor: number;
    head: number;
  };
}

interface Template {
  before: string;
  after: string;
  placeholder: string;
  select: "content" | "before-placeholder";
}

const TEMPLATES: Record<
  Exclude<AsciiDocCommand, "title" | "section" | "list">,
  Template
> = {
  link: {
    before: "https://example.com[",
    after: "]",
    placeholder: "リンク",
    select: "before-placeholder",
  },
  "code-block": {
    before: "[source,text]\n----\n",
    after: "\n----",
    placeholder: "コード",
    select: "content",
  },
  "inline-math": {
    before: "stem:[",
    after: "]",
    placeholder: "x",
    select: "content",
  },
  "block-math": {
    before: "[latexmath]\n++++\n",
    after: "\n++++",
    placeholder: "x",
    select: "content",
  },
  "note-reference": {
    before: "xref:note:NOTE_ID[",
    after: "]",
    placeholder: "参照",
    select: "before-placeholder",
  },
};

export function asciiDocCommandEdit(
  command: AsciiDocCommand,
  source: string,
  anchor: number,
  head: number,
): TextEdit {
  const from = Math.min(anchor, head);
  const to = Math.max(anchor, head);
  const selected = source.slice(from, to);
  switch (command) {
    case "title":
      return prefixLines(from, to, selected, "= ", "題名");
    case "section":
      return prefixLines(from, to, selected, "== ", "節題");
    case "list":
      return prefixLines(from, to, selected, "* ", "項目");
    default:
      return applyTemplate(from, to, selected, TEMPLATES[command]);
  }
}

function prefixLines(
  from: number,
  to: number,
  selected: string,
  prefix: string,
  placeholder: string,
): TextEdit {
  if (selected === "") {
    return {
      from,
      to,
      insert: `${prefix}${placeholder}`,
      selection: {
        anchor: from + prefix.length,
        head: from + prefix.length + placeholder.length,
      },
    };
  }
  const insert = selected
    .split("\n")
    .map((line) => `${prefix}${line}`)
    .join("\n");
  return {
    from,
    to,
    insert,
    selection: {
      anchor: from,
      head: from + insert.length,
    },
  };
}

function applyTemplate(
  from: number,
  to: number,
  selected: string,
  template: Template,
): TextEdit {
  const content = selected || template.placeholder;
  const insert = `${template.before}${content}${template.after}`;
  if (template.select === "before-placeholder") {
    const placeholder = template.before.includes("NOTE_ID")
      ? "NOTE_ID"
      : "https://example.com";
    const placeholderStart = from + template.before.indexOf(placeholder);
    return {
      from,
      to,
      insert,
      selection: {
        anchor: placeholderStart,
        head: placeholderStart + placeholder.length,
      },
    };
  }
  return {
    from,
    to,
    insert,
    selection: {
      anchor: from + template.before.length,
      head: from + template.before.length + content.length,
    },
  };
}
