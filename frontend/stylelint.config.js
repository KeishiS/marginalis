/**
 * 目盛りから外れた値を拒否する。
 *
 * 色、余白、文字の大きさは`src/styles/tokens.css`の変数から選ぶ。直書きを許すと、
 * 区別のつかない値が少しずつ増える。CodeMirrorなど外部の部品へ合わせる箇所だけ、
 * 該当行に理由を書いて`stylelint-disable-next-line`で外す。
 */
export default {
  extends: ["stylelint-config-standard"],
  plugins: ["stylelint-declaration-strict-value"],
  rules: {
    "scale-unlimited/declaration-strict-value": [
      [
        "/color/",
        "padding",
        "padding-block",
        "padding-inline",
        "padding-top",
        "padding-bottom",
        "padding-left",
        "padding-right",
        "margin",
        "margin-block",
        "margin-inline",
        "gap",
        "row-gap",
        "column-gap",
        "font-size",
        "border-radius",
        "box-shadow",
        "border",
        "border-block",
        "border-block-end",
        "border-block-start",
        "border-inline",
        "border-inline-end",
        "border-inline-start",
      ],
      {
        // 単独の変数だけを認める。混ぜた値を許すと目盛り外の値が紛れ込む。
        // clampやcolor-mixを使う数か所は、その行に理由を書いて個別に外す。
        ignoreValues: [
          "/^var\\(--/",
          // 境界線の「太さ 種類 色」のうち、種類は目盛りの対象ではない。
          "solid",
          "dashed",
          "auto",
          "inherit",
          "currentcolor",
          "transparent",
          "none",
          "0",
        ],
      },
    ],
    // 目盛りの定義そのものは直値で書く。
    "custom-property-pattern": null,
    "selector-class-pattern": null,
    "no-descending-specificity": null,
    // 書体名は原綴りを保つ。
    "value-keyword-case": [
      "lower",
      { ignoreProperties: ["font-family", "font"] },
    ],
  },
  overrides: [
    {
      // 目盛りそのものを定義するため、この一つのファイルだけ直値で書く。
      files: ["src/styles/tokens.css"],
      rules: { "scale-unlimited/declaration-strict-value": null },
    },
  ],
  ignoreFiles: ["dist/**", "node_modules/**"],
};
