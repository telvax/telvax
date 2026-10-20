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

test("the icon theme gives .tvx files the SVG icon", () => {
  const root = path.join(__dirname, "..");
  const manifest = JSON.parse(fs.readFileSync(path.join(root, "package.json"), "utf8"));
  const [theme] = manifest.contributes.iconThemes;
  const file = path.join(root, theme.path);
  const def = JSON.parse(fs.readFileSync(file, "utf8"));
  const icon = def.iconDefinitions[def.fileExtensions.tvx];
  assert.ok(icon, "tvx has no icon definition");
  assert.ok(fs.existsSync(path.join(path.dirname(file), icon.iconPath)), `${icon.iconPath} is missing`);
  const light = def.iconDefinitions[def.light.fileExtensions.tvx];
  assert.ok(fs.existsSync(path.join(path.dirname(file), light.iconPath)), `${light.iconPath} is missing`);
});
