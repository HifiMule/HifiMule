import assert from "node:assert/strict";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, utimesSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { acquireRuntimeLock } from "../runtime-lock.mjs";

test("runtime lock replaces a lock older than one hour and releases ownership", (t) => {
  const scratch = mkdtempSync(join(tmpdir(), "hifimule-runtime-lock-"));
  t.after(() => rmSync(scratch, { recursive: true, force: true }));
  const path = join(scratch, "runtime.lock");
  mkdirSync(path);
  writeFileSync(join(path, "stale-owner"), "old");
  const stale = new Date(Date.now() - 61 * 60_000);
  utimesSync(path, stale, stale);
  const unlock = acquireRuntimeLock(path);
  assert.equal(existsSync(join(path, "stale-owner")), false);
  assert.equal(JSON.parse(readFileSync(join(path, "owner.json"), "utf8")).pid, process.pid);
  unlock();
  assert.equal(existsSync(path), false);
});

test("runtime lock times out after thirty minutes without deleting a fresh lock", (t) => {
  const scratch = mkdtempSync(join(tmpdir(), "hifimule-runtime-lock-"));
  t.after(() => rmSync(scratch, { recursive: true, force: true }));
  const path = join(scratch, "runtime.lock");
  const unlock = acquireRuntimeLock(path);
  t.after(unlock);
  const now = Date.now();
  const times = [now, now, now + 30 * 60_000];
  t.mock.method(Date, "now", () => times.shift());
  assert.throws(() => acquireRuntimeLock(path), /Timed out waiting for audio runtime lock/);
  assert.equal(JSON.parse(readFileSync(join(path, "owner.json"), "utf8")).pid, process.pid);
});
