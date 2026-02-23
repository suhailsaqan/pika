import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { compareVersionsDesc, isCompatibleVersion } from "./sidecar-install.js";

describe("compareVersionsDesc", () => {
  it("sorts simple versions in descending order", () => {
    const input = ["v0.1.0", "v0.2.0", "v0.1.5"];
    const result = [...input].sort(compareVersionsDesc);
    assert.deepStrictEqual(result, ["v0.2.0", "v0.1.5", "v0.1.0"]);
  });

  it("handles v0.9.0 vs v0.10.0 correctly (not lexicographic)", () => {
    const input = ["v0.9.0", "v0.10.0", "v0.2.0"];
    const result = [...input].sort(compareVersionsDesc);
    assert.deepStrictEqual(result, ["v0.10.0", "v0.9.0", "v0.2.0"]);
  });

  it("handles pikachat-v prefixed tags", () => {
    const input = ["pikachat-v0.4.0", "pikachat-v0.5.1", "pikachat-v0.5.0"];
    const result = [...input].sort(compareVersionsDesc);
    assert.deepStrictEqual(result, ["pikachat-v0.5.1", "pikachat-v0.5.0", "pikachat-v0.4.0"]);
  });

  it("handles major version differences", () => {
    const input = ["v1.0.0", "v2.0.0", "v0.9.0"];
    const result = [...input].sort(compareVersionsDesc);
    assert.deepStrictEqual(result, ["v2.0.0", "v1.0.0", "v0.9.0"]);
  });

  it("handles patch version differences", () => {
    const input = ["v0.1.1", "v0.1.3", "v0.1.2"];
    const result = [...input].sort(compareVersionsDesc);
    assert.deepStrictEqual(result, ["v0.1.3", "v0.1.2", "v0.1.1"]);
  });

  it("keeps equal versions stable", () => {
    const input = ["v0.5.0", "v0.5.0"];
    const result = [...input].sort(compareVersionsDesc);
    assert.deepStrictEqual(result, ["v0.5.0", "v0.5.0"]);
  });

  it("handles empty array", () => {
    const result = ([] as string[]).sort(compareVersionsDesc);
    assert.deepStrictEqual(result, []);
  });
});

describe("isCompatibleVersion", () => {
  it("accepts same major.minor with different patch", () => {
    assert.strictEqual(isCompatibleVersion("pikachat-v0.5.1", "0.5.0"), true);
    assert.strictEqual(isCompatibleVersion("pikachat-v0.5.2", "0.5.0"), true);
    assert.strictEqual(isCompatibleVersion("pikachat-v0.5.0", "0.5.0"), true);
    assert.strictEqual(isCompatibleVersion("pikachat-v0.5.10", "0.5.3"), true);
  });

  it("rejects different minor version", () => {
    assert.strictEqual(isCompatibleVersion("pikachat-v0.6.0", "0.5.0"), false);
    assert.strictEqual(isCompatibleVersion("pikachat-v0.4.0", "0.5.0"), false);
    assert.strictEqual(isCompatibleVersion("pikachat-v0.6.1", "0.5.9"), false);
  });

  it("rejects different major version", () => {
    assert.strictEqual(isCompatibleVersion("pikachat-v1.5.0", "0.5.0"), false);
    assert.strictEqual(isCompatibleVersion("pikachat-v2.0.0", "1.0.0"), false);
  });

  it("accepts exact same version", () => {
    assert.strictEqual(isCompatibleVersion("pikachat-v0.5.1", "0.5.1"), true);
  });

  it("works with v-prefixed plugin versions", () => {
    assert.strictEqual(isCompatibleVersion("pikachat-v0.5.1", "v0.5.0"), true);
    assert.strictEqual(isCompatibleVersion("pikachat-v0.6.0", "v0.5.0"), false);
  });

  it("works with bare version strings (no prefix)", () => {
    assert.strictEqual(isCompatibleVersion("v0.5.1", "0.5.0"), true);
    assert.strictEqual(isCompatibleVersion("v0.6.0", "0.5.0"), false);
  });
});
