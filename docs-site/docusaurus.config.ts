import type * as Preset from "@docusaurus/preset-classic";
import type { Config } from "@docusaurus/types";
import { themes as prismThemes } from "prism-react-renderer";
import rehypeKatex from "rehype-katex";
import remarkMath from "remark-math";
import githubLinks from "./src/plugins/github-links";

const github = "https://github.com/Tesserax-foundation/tesserax";

const config: Config = {
  title: "Tesserax",
  tagline: "The post-quantum chain for AI agents",
  favicon: "img/favicon-32.png",
  url: "https://docs.tesserax.app",
  baseUrl: "/",
  trailingSlash: false,
  onBrokenLinks: "throw",
  markdown: {
    // The guides of docs/ are plain Markdown, read on GitHub too.
    format: "detect",
    mermaid: true,
    hooks: { onBrokenMarkdownLinks: "throw" },
  },
  i18n: { defaultLocale: "en", locales: ["en"] },
  themes: [
    "@docusaurus/theme-mermaid",
    [
      "@easyops-cn/docusaurus-search-local",
      { hashed: true, docsDir: "../docs", docsRouteBasePath: "/", indexBlog: false, highlightSearchTermsOnTargetPage: true },
    ],
  ],
  presets: [
    [
      "classic",
      {
        docs: {
          path: "../docs",
          routeBasePath: "/",
          sidebarPath: "./sidebars.ts",
          editUrl: `${github}/edit/main/docs/`,
          beforeDefaultRemarkPlugins: [githubLinks],
          remarkPlugins: [remarkMath],
          rehypePlugins: [rehypeKatex],
          showLastUpdateTime: false,
        },
        blog: false,
        pages: false,
        theme: { customCss: "./src/css/custom.css" },
      } satisfies Preset.Options,
    ],
  ],
  themeConfig: {
    image: "img/og.png",
    colorMode: { defaultMode: "dark", respectPrefersColorScheme: false },
    navbar: {
      title: "Tesserax",
      logo: { alt: "Tesserax", src: "img/logo.png", href: "https://tesserax.app", target: "_self" },
      items: [
        { to: "/whitepaper", label: "Whitepaper", position: "left" },
        { to: "/lightpaper", label: "Lightpaper", position: "left" },
        { to: "/agents", label: "Guides", position: "left" },
        { href: "https://wallet.tesserax.app", label: "Wallet", position: "right" },
        { href: "https://testnet-explorer.tesserax.app", label: "Explorer", position: "right" },
        { href: github, label: "GitHub", position: "right" },
      ],
    },
    footer: {
      style: "dark",
      links: [
        {
          title: "Read",
          items: [
            { label: "Whitepaper", to: "/whitepaper" },
            { label: "Lightpaper", to: "/lightpaper" },
            { label: "Bug bounty", to: "/bug-bounty" },
          ],
        },
        {
          title: "Use",
          items: [
            { label: "Wallet", href: "https://wallet.tesserax.app" },
            { label: "Explorer", href: "https://testnet-explorer.tesserax.app" },
            { label: "Faucet", href: "https://faucet.tesserax.app" },
            { label: "Mining pool", href: "https://testnet-pool.tesserax.app" },
          ],
        },
        {
          title: "Build",
          items: [
            { label: "GitHub", href: github },
            { label: "Miner", href: "https://github.com/Tesserax-foundation/tesserax-miner" },
            { label: "Website", href: "https://tesserax.app" },
          ],
        },
      ],
      copyright: "Tesserax developers · Apache-2.0 and GPL-3.0 · The testnet has no value.",
    },
    prism: {
      theme: prismThemes.github,
      darkTheme: prismThemes.oneDark,
      additionalLanguages: ["rust", "toml", "bash", "solidity", "json", "python"],
    },
    mermaid: {
      theme: { light: "neutral", dark: "dark" },
    },
  } satisfies Preset.ThemeConfig,
};

export default config;
