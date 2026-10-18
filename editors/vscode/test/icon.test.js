const test = require("node:test");
const assert = require("node:assert");
const fs = require("node:fs");
const path = require("node:path");

test("the manifest names a square PNG icon of at least 128 pixels", () => {
  const manifest = JSON.parse(fs.readFileSync(path.join(__dirname, "..", "package.json"), "utf8"));
  assert.ok(manifest.icon, "package.json has no icon");
  const png = fs.readFileSync(path.join(__dirname, "..", manifest.icon));
  assert.strictEqual(png.subarray(1, 4).toString(), "PNG");
  const width = png.readUInt32BE(16);
  const height = png.readUInt32BE(20);
  assert.strictEqual(width, height);
  assert.ok(width >= 128, `icon is ${width}px`);
});
