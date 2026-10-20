/** Timestamped logs on standard error. `LOG_LEVEL=debug` shows every block. */

const levels = { debug: 0, info: 1, warn: 2, error: 3 } as const;
type Level = keyof typeof levels;
const threshold = levels[(process.env.LOG_LEVEL as Level) ?? "info"] ?? levels.info;

const write = (level: Level, message: string) => {
  if (levels[level] >= threshold) process.stderr.write(`${new Date().toISOString()} ${level.padEnd(5)} ${message}\n`);
};

export const log = {
  debug: (message: string) => write("debug", message),
  info: (message: string) => write("info", message),
  warn: (message: string) => write("warn", message),
  error: (message: string) => write("error", message),
};
