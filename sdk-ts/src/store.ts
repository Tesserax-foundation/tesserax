/** JSON state files, written atomically: a crash leaves either the previous or the new content,
 * never a partial file. Node.js only: `node:fs` is loaded when a state file is used. */

/** A state file cannot be read or written, or belongs to another chain or key. */
export class StorageError extends Error {
  override name = "StorageError";
}

const fsModule = () => import("node:fs/promises");

/** The JSON content of the file at `file`, or `undefined` if it does not exist. */
export async function loadJson(file: string): Promise<unknown> {
  const fs = await fsModule();
  let text: string;
  try {
    text = await fs.readFile(file, "utf8");
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === "ENOENT") return undefined;
    throw new StorageError(`cannot read ${file}: ${(error as Error).message}`);
  }
  try {
    return JSON.parse(text);
  } catch (error) {
    throw new StorageError(`${file} is not a valid state file: ${(error as Error).message}`);
  }
}

/** Replace the file at `file` with `value` as JSON, readable by the user only: written to a
 * temporary file in the same directory, flushed to disk, then renamed over `file`. */
export async function saveJson(file: string, value: unknown): Promise<void> {
  const fs = await fsModule();
  const path = await import("node:path");
  const temporary = path.join(path.dirname(file), `.${path.basename(file)}.tmp`);
  try {
    const handle = await fs.open(temporary, "w", 0o600);
    try {
      await handle.writeFile(JSON.stringify(value, null, 2));
      await handle.sync();
    } finally {
      await handle.close();
    }
    await fs.rename(temporary, file);
    if (process.platform !== "win32") {
      // Make the rename itself durable.
      const directory = await fs.open(path.dirname(file), "r");
      try {
        await directory.sync();
      } finally {
        await directory.close();
      }
    }
  } catch (error) {
    throw new StorageError(`cannot write ${file}: ${(error as Error).message}`);
  }
}
