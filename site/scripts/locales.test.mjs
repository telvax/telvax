import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { LOCALES, availableLocales, fill, localeDir, translationsDir } from './locales.mjs';

function withFolder(fn) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'telvax-translations-'));
  const before = process.env.TELVAX_TRANSLATIONS;
  process.env.TELVAX_TRANSLATIONS = dir;
  try {
    fn(dir);
  } finally {
    if (before === undefined) delete process.env.TELVAX_TRANSLATIONS;
    else process.env.TELVAX_TRANSLATIONS = before;
    fs.rmSync(dir, { recursive: true, force: true });
  }
}

test('a language is built only when its repository has the tour', () => {
  withFolder((dir) => {
    assert.equal(translationsDir(), dir);
    assert.deepEqual(availableLocales(), []);
    const es = LOCALES.find((l) => l.code === 'es');
    fs.mkdirSync(path.join(localeDir(es), 'docs', 'guide'), { recursive: true });
    assert.deepEqual(availableLocales(), [], 'a repository without the tour is left out');
    fs.writeFileSync(path.join(localeDir(es), 'docs', 'guide', 'tour.md'), '# Un recorrido\n');
    assert.deepEqual(availableLocales().map((l) => l.code), ['es']);
  });
});

test('every language has the labels of the whole sidebar', () => {
  const keys = Object.keys(LOCALES[0].sidebar).sort();
  for (const l of LOCALES) {
    assert.deepEqual(Object.keys(l.sidebar).sort(), keys, l.code);
    assert.ok(Object.values(l.sidebar).every((v) => v.trim()), `${l.code} has an empty label`);
    assert.equal(l.url, `https://github.com/telvax/${l.repo}`);
  }
});

test('every language has the texts the site writes in it, with their places', () => {
  for (const l of LOCALES) {
    assert.ok(l.texts.license.trim(), `${l.code}: the note of the licenses`);
    assert.ok(l.texts.olderReleases.includes('{version}') && l.texts.olderReleases.includes('({link})'), l.code);
    assert.ok(l.texts.reference.includes('{n}') && l.texts.reference.includes('{total}'), l.code);
  }
  assert.equal(fill('{n} of {total}, {other}', { n: 3, total: 9 }), '3 of 9, {other}');
});

