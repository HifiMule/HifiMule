import assert from "node:assert/strict";
import test from "node:test";
import {
  audioRuntimeVerification,
  verifyAudioRuntime,
  withAudioRuntimeVerification,
  withoutAudioRuntimeVerification,
} from "../verify-audio-runtime.mjs";

const exactVersions = {
  libavcodec: "63.1.100",
  libavformat: "63.1.100",
  libavutil: "61.1.100",
  libswresample: "7.1.100",
};

function pkgConfigFixture(overrides = {}) {
  const versions = { ...exactVersions, ...overrides };
  return (_command, args, options) => {
    assert.equal(options.env[audioRuntimeVerification.environmentVariable], undefined);
    if (args[0] === "--modversion") return `${versions[args[1]]}\n`;
    if (args[0] === "--variable=libdir") return "/controlled/ffmpeg/lib\n";
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
      execFileSync: pkgConfigFixture({ libavcodec: "63.1.101" }),
    }),
    /libavcodec ABI mismatch: expected exact ABI 63\.1\.100, found 63\.1\.101/,
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
    /expected ABI 63\.1\.100 or newer within major 63, found 63\.0\.99/,
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
