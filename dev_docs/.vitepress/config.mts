import { defineConfig } from "vitepress";

export default defineConfig({
  lang: "ja-JP",
  title: "Oden Internals",
  description: "Oden の設計・コードベース・開発フローを読むための技術ドキュメント",
  base: "/oden/",
  cleanUrls: true,
  lastUpdated: false,
  head: [
    ["meta", { name: "theme-color", content: "#3178c6" }],
  ],
  themeConfig: {
    siteTitle: "Oden Internals",
    nav: [
      { text: "はじめに", link: "/guide/getting-started" },
      { text: "アーキテクチャ", link: "/architecture/overview" },
      { text: "独自機能", link: "/features/oden" },
      { text: "開発", link: "/development/testing" },
    ],
    sidebar: [
      {
        text: "導入",
        items: [{ text: "はじめに", link: "/guide/getting-started" }],
      },
      {
        text: "内部アーキテクチャ",
        items: [
          { text: "全体像", link: "/architecture/overview" },
          { text: "CLI とサブコマンド", link: "/architecture/cli" },
          { text: "実行ライフサイクル", link: "/architecture/execution-flow" },
          { text: "Runtime・Ops・権限", link: "/architecture/runtime" },
          { text: "Extensions・Ops の詳細", link: "/architecture/ops-permissions" },
          { text: "モジュールとパッケージ", link: "/architecture/modules" },
          { text: "解決・取得・ロードの流れ", link: "/architecture/module-pipeline" },
        ],
      },
      {
        text: "Oden 独自機能",
        items: [
          { text: "Deno からの変更点", link: "/features/oden" },
          { text: "Turso SQLite の内部", link: "/features/sqlite" },
          { text: "Desktop の内部", link: "/features/desktop" },
        ],
      },
      {
        text: "コントリビューション",
        items: [
          { text: "変更箇所を探す", link: "/development/navigation" },
          { text: "ビルド・テスト・CI", link: "/development/testing" },
          { text: "新機能の実装", link: "/development/adding-features" },
          { text: "デバッグ", link: "/development/debugging" },
        ],
      },
    ],
    search: { provider: "local" },
    outline: { level: [2, 3], label: "このページ" },
    editLink: {
      pattern: "https://github.com/taisan11/oden/edit/main/dev_docs/:path",
      text: "このページを GitHub で編集",
    },
    socialLinks: [{ icon: "github", link: "https://github.com/taisan11/oden" }],
    footer: {
      message: "Deno を基にした実験的フォーク。非公式の開発者向け資料です。",
      copyright: "MIT License",
    },
  },
});
