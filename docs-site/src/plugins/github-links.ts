// The guides of docs/ link to files of the repository with relative paths, such as
// `../swaps/evm/README.md`: on the site, they point to those files on GitHub.

import type { Link, Root } from "mdast";
import { visit } from "unist-util-visit";

const BASE = "https://github.com/Tesserax-foundation/tesserax/blob/main/";

export default function githubLinks() {
  return (tree: Root) => {
    visit(tree, "link", (node: Link) => {
      if (node.url.startsWith("../")) node.url = BASE + node.url.slice(3);
    });
  };
}
