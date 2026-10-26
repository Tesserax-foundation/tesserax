// The code samples of the developers section, highlighted with a small tokenizer, the first one
// typed out when it comes into view.

const SAMPLES: Record<string, string> = {
  ts: `import { Amount, Keypair, TesseraxClient } from "@tesserax/sdk";

const client = new TesseraxClient("ws://127.0.0.1:9944");

// An owner delegates 10 TESS a day, at most 2 per payment, to an agent key.
const owner = Keypair.fromUri(process.env.OWNER_KEY!);
const { keypair: agent } = Keypair.generate();
await client.transfer(owner, agent.address, Amount.fromTess("0.5")); // the agent's fees
await client.authorizeAgent(owner, agent.address, {
  budget: Amount.fromTess("10"),
  maxPerCall: Amount.fromTess("2"),
});

// The agent finds a translator and pays it from the owner's funds.
const [translator] = await client.findAgents("translation");
await client.agentPayAgent(agent, owner.address, translator.address, Amount.fromTess("0.25"));`,
  py: `from tesserax import Amount, Client, Keypair, Policy

client = Client("ws://127.0.0.1:9944")

# An owner delegates 10 TESS a day, at most 2 per payment, to an agent key.
owner = Keypair.from_uri(open("owner.key").read())
agent, agent_phrase = Keypair.generate()
client.transfer(owner, agent.address, Amount.from_tess("0.5"))  # the agent's fees
client.authorize_agent(owner, agent.address, Policy(budget=Amount.from_tess("10"), max_per_call=Amount.from_tess("2")))

# The agent finds a translator and pays it from the owner's funds.
translator = client.find_agents("translation")[0]
client.agent_pay_agent(agent, owner.address, translator.address, Amount.from_tess("0.25"))
print(client.delegation(owner.address, agent.address).remaining)`,
  rs: `use tesserax_sdk::{Amount, Client, Keypair, Permissions, Policy};

let client = Client::connect("ws://127.0.0.1:9944").await?;

// An owner delegates 10 TESS a day, at most 2 per payment, to an agent key.
let owner = Keypair::from_uri("<24-word phrase>")?;
let (agent, agent_phrase) = Keypair::generate()?;
client.transfer(&owner, &agent.account_id(), Amount::from_tess("0.5")?).await?; // agent fees
client
    .authorize_agent(&owner, &agent.account_id(), &Policy {
        budget: Amount::from_tess("10")?,
        period_blocks: 4_800,
        max_per_call: Amount::from_tess("2")?,
        expires_at: None,
        permissions: Permissions { transfers: true, contracts: false },
        targets: vec![],
    })
    .await?;`,
  mcp: `{
  "mcpServers": {
    "tesserax": {
      "command": "/path/to/tesserax-mcp",
      "env": {
        "TESSERAX_URL": "ws://127.0.0.1:9944",
        "TESSERAX_KEY_FILE": "/path/to/agent.key",
        "TESSERAX_OWNER": "te..."
      }
    }
  }
}`,
};

const KEYWORDS = new Set([
  "import", "from", "const", "let", "await", "new", "use", "print", "open", "None", "true", "false",
  "vec", "async", "fn", "return",
]);

type Token = [cls: string, text: string];

function tokenize(source: string, language: string): Token[] {
  const comment = language === "py" ? "#" : "//";
  const tokens: Token[] = [];
  const pattern = new RegExp(
    `(${comment === "#" ? "#" : "\\/\\/"}[^\\n]*)|("(?:[^"\\\\]|\\\\.)*")|(\\b\\d[\\d_.]*\\b)|([A-Za-z_][A-Za-z0-9_]*)(?=\\s*[(!])|([A-Za-z_][A-Za-z0-9_]*)|([\\s\\S])`,
    "g",
  );
  for (const match of source.matchAll(pattern)) {
    const [text, isComment, isString, isNumber, isCall, isWord] = match;
    if (isComment) tokens.push(["c", text]);
    else if (isString) tokens.push(["s", text]);
    else if (isNumber) tokens.push(["n", text]);
    else if (isCall) tokens.push([KEYWORDS.has(text) ? "k" : "f", text]);
    else if (isWord) tokens.push([KEYWORDS.has(text) ? "k" : "", text]);
    else tokens.push(["", text]);
  }
  return tokens;
}

function escape(text: string): string {
  return text.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
}

/** The highlighted HTML of the first `limit` characters of `tokens`. */
function render(tokens: Token[], limit = Infinity): string {
  let html = "";
  let left = limit;
  for (const [cls, text] of tokens) {
    if (left <= 0) break;
    const part = escape(text.slice(0, left));
    left -= text.length;
    html += cls ? `<span class="${cls}">${part}</span>` : part;
  }
  return html;
}

export function startCode(reducedMotion: boolean): void {
  const blocks = new Map<string, HTMLElement>();
  for (const pre of document.querySelectorAll<HTMLElement>("[data-code]")) {
    const language = pre.dataset.code!;
    blocks.set(language, pre);
    pre.innerHTML = render(tokenize(SAMPLES[language], language));
  }

  const tabs = document.querySelectorAll<HTMLButtonElement>("[data-tab]");
  for (const tab of tabs) {
    tab.addEventListener("click", () => {
      for (const other of tabs) other.setAttribute("aria-selected", String(other === tab));
      for (const [language, pre] of blocks) pre.hidden = language !== tab.dataset.tab;
    });
  }

  const first = blocks.get("ts");
  if (!first || reducedMotion) return;
  const tokens = tokenize(SAMPLES.ts, "ts");
  const total = SAMPLES.ts.length;
  first.innerHTML = '<span class="caret"></span>';
  const observer = new IntersectionObserver(([entry]) => {
    if (!entry.isIntersecting) return;
    observer.disconnect();
    const begin = performance.now();
    const step = (now: number) => {
      const shown = Math.min(total, Math.floor((now - begin) * 0.45));
      first.innerHTML = render(tokens, shown) + (shown < total ? '<span class="caret"></span>' : "");
      if (shown < total) requestAnimationFrame(step);
    };
    requestAnimationFrame(step);
  }, { threshold: 0.4 });
  observer.observe(first);
}
