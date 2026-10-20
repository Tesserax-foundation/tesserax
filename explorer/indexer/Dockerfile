FROM node:22-alpine
WORKDIR /app
COPY package.json ./
RUN npm install --omit=dev --no-audit --no-fund && npm install --no-save tsx@4.23.15
COPY src ./src
USER node
CMD ["npx", "tsx", "src/main.ts"]
