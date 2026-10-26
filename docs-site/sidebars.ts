import type { SidebarsConfig } from "@docusaurus/plugin-content-docs";

const sidebars: SidebarsConfig = {
  docs: [
    "README",
    "lightpaper",
    {
      type: "category",
      label: "Whitepaper",
      link: { type: "doc", id: "whitepaper/index" },
      collapsed: false,
      items: [
        "whitepaper/introduction",
        "whitepaper/architecture",
        "whitepaper/cryptography",
        "whitepaper/consensus",
        "whitepaper/economics",
        "whitepaper/agents",
        "whitepaper/privacy",
        "whitepaper/contracts",
        "whitepaper/swaps",
        "whitepaper/governance",
        "whitepaper/security",
        "whitepaper/roadmap",
        "whitepaper/references",
      ],
    },
    {
      type: "category",
      label: "Guides",
      collapsed: false,
      items: ["agents", "micropayments", "privacy", "contracts", "swaps", "governance"],
    },
    {
      type: "category",
      label: "Network",
      collapsed: false,
      items: ["node", "testnet", "mining", "pool", "rpc", "mainnet", "bug-bounty"],
    },
  ],
};

export default sidebars;
