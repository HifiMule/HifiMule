import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { join, resolve } from "node:path";
import {
  audioRuntimeVerification,
  verifyAudioRuntime,
  withAudioRuntimeVerification,
  withoutAudioRuntimeVerification,
} from "../verify-audio-runtime.mjs";

const exactVersions = {
  libavcodec: "63.1.101",
  libavformat: "63.1.101",
  libavutil: "61.1.101",
  libswresample: "7.1.101",
};

function pkgConfigFixture(overrides = {}, libdir = join(resolve("/controlled/ffmpeg"), "lib")) {
  const versions = { ...exactVersions, ...overrides };
  return (_command, args, options) => {
    assert.equal(options.env[audioRuntimeVerification.environmentVariable], undefined);
    if (args[0] === "--modversion") return `${versions[args[1]]}\n`;
    if (args[0] === "--variable=libdir") return `${libdir}\n`;
    throw new Error(`Unexpected pkg-config arguments: ${args.join(" ")}`);
  };
}

test("exact verifier accepts every manifest ABI and strips inherited authority", () => {
  const result = verifyAudioRuntime({
    prefix: "/controlled/ffmpeg",
    env: { [audioRuntimeVerification.environmentVariable]: audioRuntimeVerification.value },
    execFileSync: pkgConfigFixture(),
  });
  assert.deepEqual(result.versions, exactVersions);
});

test("controlled-prefix verifier rejects same-major but non-manifest ABI versions", () => {
  assert.throws(
    () => verifyAudioRuntime({
      prefix: "/controlled/ffmpeg",
      execFileSync: pkgConfigFixture({ libavcodec: "63.1.102" }),
    }),
    /libavcodec ABI mismatch: expected exact ABI 63\.1\.101, found 63\.1\.102/,
  );
});

test("host verifier accepts the established macOS micro version within the exact ABI major", () => {
  const result = verifyAudioRuntime({
    execFileSync: pkgConfigFixture({
      libavcodec: "63.1.101",
      libavformat: "63.1.101",
      libavutil: "61.1.101",
      libswresample: "7.1.101",
    }),
  });
  assert.equal(result.versions.libavcodec, "63.1.101");
});

test("host verifier rejects an older version even when the ABI major matches", () => {
  assert.throws(
    () => verifyAudioRuntime({
      execFileSync: pkgConfigFixture({ libavcodec: "63.0.99" }),
    }),
    /expected ABI 63\.1\.101 or newer within major 63, found 63\.0\.99/,
  );
});

test("verification marker helpers sanitize before granting authority", () => {
  const inherited = {
    KEEP: "yes",
    [audioRuntimeVerification.environmentVariable]: "forged",
  };
  assert.deepEqual(withoutAudioRuntimeVerification(inherited), { KEEP: "yes" });
  assert.deepEqual(withAudioRuntimeVerification(inherited), {
    KEEP: "yes",
    [audioRuntimeVerification.environmentVariable]: audioRuntimeVerification.value,
  });
});

test("controlled-prefix verifier rejects parent and similarly named sibling directories", () => {
  const prefix = resolve("/controlled/ffmpeg");
  for (const libdir of [resolve(prefix, ".."), resolve(prefix, "../ffmpeg-other/lib"), resolve(prefix, "../unrelated/lib")]) {
    assert.throws(
      () => verifyAudioRuntime({ prefix, execFileSync: pkgConfigFixture({}, libdir) }),
      /resolved outside controlled prefix/,
    );
  }
});

test("controlled-prefix verifier accepts the prefix itself and nested native paths", () => {
  const prefix = resolve("/controlled/ffmpeg");
  for (const libdir of [prefix, join(prefix, "lib", "nested")]) {
    assert.deepEqual(verifyAudioRuntime({ prefix, execFileSync: pkgConfigFixture({}, libdir) }).versions, exactVersions);
  }
});

test("runtime manifest records the bounded production buffering policy", () => {
  const manifest = JSON.parse(readFileSync(
    new URL("../../hifimule-daemon/audio-runtime.json", import.meta.url),
    "utf8",
  ));
  assert.deepEqual(manifest.bufferPolicy, {
    compressedChunkBytes: 65536,
    compressedCapacityBytes: 8388608,
    compressedAggregateCapacityBytes: 16777216,
    networkChunkCapacityBytes: 1048576,
    compressedWindowBytes: 7274496,
    preparationDeadlineMilliseconds: 60000,
    refillMilliseconds: 100,
    pcmTargetMilliseconds: 500,
    pcmCapacityMaxBytes: 1048576,
    pcmAggregateCapacityMaxBytes: 2097152,
    sourceSlotLimit: 2,
    startupFillMilliseconds: 100,
  });
});
