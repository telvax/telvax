import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
import test from 'node:test';
import { isVersion, landing, notFoundPage, rootPage, seriesOf, upsert } from './publish.mjs';

test('a tag belongs to a series', () => {
  assert.equal(seriesOf('v0.30.1'), '0.30');
  assert.equal(seriesOf('1.2.0'), '1.2');
  assert.equal(seriesOf('v1.0.0-rc.1'), '1.0');
  assert.throws(() => seriesOf('latest'), /not a version/);
});

test('only dev and series are versions', () => {
  assert.ok(isVersion('dev') && isVersion('0.30') && isVersion('12.4'));
  for (const bad of ['', '0', '0.30.1', 'v0.30', '../x', 'dev/x', 'main']) assert.ok(!isVersion(bad), bad);
});

test('releases come newest first, then dev, and the newest release is the latest', () => {
  let list = upsert([], 'dev');
  assert.deepEqual(list.map((v) => v.version), ['dev']);
  assert.equal(landing(list), 'dev');
  list = upsert(list, '0.9');
  list = upsert(list, '0.30');
  list = upsert(list, '0.10');
  assert.deepEqual(list.map((v) => v.version), ['0.30', '0.10', '0.9', 'dev']);
  assert.deepEqual(list.map((v) => v.latest), [true, false, false, false]);
  assert.equal(list[0].label, '0.30 (latest)');
  assert.equal(landing(list), '0.30');
  // Publishing a series again changes nothing.
  assert.deepEqual(upsert(list, '0.10'), list);
});

test('the root pages point at the right place', () => {
  assert.match(rootPage('/telvax', '0.30'), /url=\/telvax\/0\.30\//);
  assert.match(notFoundPage('/telvax'), /const root = "\/telvax"/);
});

test('publishing copies the site and writes the files of the root', () => {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'telvax-pages-'));
  const dist = path.join(tmp, 'dist');
  fs.mkdirSync(path.join(dist, '_astro'), { recursive: true });
  fs.writeFileSync(path.join(dist, 'index.html'), 'home');
  const pages = path.join(tmp, 'pages');
  fs.mkdirSync(pages);
  fs.mkdirSync(path.join(pages, '0.30'));
  fs.writeFileSync(path.join(pages, '0.30', 'old.html'), 'old');
  fs.writeFileSync(path.join(pages, 'versions.json'), JSON.stringify([{ version: '0.30' }]));
  const run = (version) =>
    execFileSync('node', [path.resolve('scripts/publish.mjs'), '--pages', pages, '--dist', dist, '--version', version], {
      encoding: 'utf8',
    });
  run('0.30');
  assert.ok(!fs.existsSync(path.join(pages, '0.30', 'old.html')), 'a version is replaced, not merged');
  assert.ok(fs.existsSync(path.join(pages, '0.30', '_astro')));
  run('dev');
  const versions = JSON.parse(fs.readFileSync(path.join(pages, 'versions.json'), 'utf8'));
  assert.deepEqual(versions.map((v) => v.version), ['0.30', 'dev']);
  assert.match(fs.readFileSync(path.join(pages, 'index.html'), 'utf8'), /\/telvax\/0\.30\//);
  assert.ok(fs.existsSync(path.join(pages, '.nojekyll')) && fs.existsSync(path.join(pages, '404.html')));
  assert.throws(() => run('../evil'), /not "dev" or a series/);
  assert.ok(!fs.existsSync(path.join(tmp, 'evil')));
});
