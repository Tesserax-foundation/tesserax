# Tesserax website

The website of Tesserax, served at https://tesserax.app, built with Vite:

- `index.html`: the landing page, with the live best block of the testnet (from its public RPC) and
  its supply (from the explorer API);
- `mine.html`, served at `/mine`: the mining guide in three steps, with the commands for the
  visitor's system and address, and the live figures of the pool (from its API).

```bash
npm install
npm run dev     # http://localhost:5173
npm run build   # dist/
```

`vercel.json` sets its content security policy and headers.
