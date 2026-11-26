import assert from 'node:assert/strict';
import test from 'node:test';
import { readSegments, renderReference, todo } from './std-catalog.mjs';

const segments = [
  { md: '# ' },
  { key: 'page.title', en: 'Standard library', hash: 'aaaa' },
  { md: '\n\n#### `std.text.trim`\n\n```text\nstd.text.trim(text: Text) -> Text\n```\n\n' },
  { key: 'fn.std::text::trim', en: 'Removes whitespace at both ends.', hash: 'bbbb' },
  { md: '\n\n| ' },
  { key: 'label.method', en: 'Method', hash: 'cccc' },
  { md: ' |\n| ' },
  { key: 'label.method', en: 'Method', hash: 'cccc' },
  { md: ' |\n' },
];

test('the page of a language takes the translated texts whose English has not changed', () => {
  const catalog = {
    'page.title': { en: 'aaaa', text: 'Biblioteca estándar' },
    'fn.std::text::trim': { en: 'old0', text: 'Quita los espacios.' },
    'label.method': { en: 'cccc', text: 'Método' },
    'fn.std::gone': { en: 'dddd', text: 'Ya no existe.' },
  };
  const r = renderReference(segments, catalog);
  assert.equal(
    r.markdown,
    '# Biblioteca estándar\n\n#### `std.text.trim`\n\n```text\nstd.text.trim(text: Text) -> Text\n```\n\n' +
      'Removes whitespace at both ends.\n\n| Método |\n| Método |\n',
  );
  assert.deepEqual([r.translated, r.total], [2, 3], 'a key that appears twice counts once');
  assert.deepEqual(r.stale, ['fn.std::text::trim']);
  assert.deepEqual(r.missing, []);
  assert.deepEqual(r.unknown, ['fn.std::gone']);
  assert.deepEqual(todo(segments, catalog), {
    'fn.std::text::trim': { en: 'bbbb', text: 'Removes whitespace at both ends.' },
  });
});

test('without a catalog the page is the English one, the same as docs/reference/std.md', async () => {
  const fs = await import('node:fs');
  const real = readSegments();
  const r = renderReference(real, {});
  assert.equal(r.markdown, fs.readFileSync(new URL('../../docs/reference/std.md', import.meta.url), 'utf8'));
  assert.equal(r.translated, 0);
  assert.equal(r.missing.length, r.total);
});
