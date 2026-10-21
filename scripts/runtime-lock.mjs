import { mkdirSync, rmSync, statSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";

export function acquireRuntimeLock(path) {
  mkdirSync(dirname(path), { recursive: true });
  const deadline = Date.now() + 30 * 60_000;
  while (true) {
    try { mkdirSync(path); writeFileSync(join(path, "owner.json"), JSON.stringify({ pid: process.pid, at: new Date().toISOString() })); return () => rmSync(path, { recursive: true, force: true }); }
    catch (error) {
      if (error.code !== "EEXIST") throw error;
      if (Date.now() - statSync(path).mtimeMs > 60 * 60_000) { rmSync(path, { recursive: true, force: true }); continue; }
      if (Date.now() >= deadline) throw new Error(`Timed out waiting for audio runtime lock: ${path}`);
      Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, 500);
    }
  }
}
