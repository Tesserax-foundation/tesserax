# Indexer of Tesserax Explorer, built from the explorer directory for its lockfile:
#
#   docker build -f indexer/Dockerfile -t tesserax-indexer .
#   docker run --env-file indexer/.env tesserax-indexer

FROM docker.io/library/node:22.22.3-alpine3.22@sha256:cd7807368cf24826297cbad5dca1a44972ccfd770647db52a8c7589eb4599ac8 AS build
WORKDIR /app
COPY package.json package-lock.json ./
COPY indexer/package.json indexer/
COPY web/package.json web/
RUN npm ci --workspace indexer --no-audit --no-fund
COPY indexer/tsconfig.json indexer/
COPY indexer/src indexer/src
RUN npm run build --workspace indexer

FROM docker.io/library/node:22.22.3-alpine3.22@sha256:cd7807368cf24826297cbad5dca1a44972ccfd770647db52a8c7589eb4599ac8
ENV NODE_ENV=production
WORKDIR /app
COPY package.json package-lock.json ./
COPY indexer/package.json indexer/
COPY web/package.json web/
RUN npm ci --workspace indexer --omit=dev --no-audit --no-fund && npm cache clean --force
COPY --from=build /app/indexer/dist indexer/dist
USER node
CMD ["node", "indexer/dist/main.js"]
