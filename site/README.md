# Tesserax website

The landing page of Tesserax, served at https://tesserax.app: a static page built with Vite, with
the live best block of the testnet (from its public RPC) and its supply (from the explorer API).

```bash
npm install
npm run dev     # http://localhost:5173
npm run build   # dist/
```

`vercel.json` sets its content security policy and headers.
