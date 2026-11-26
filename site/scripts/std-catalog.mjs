// The library reference in a language (#232).
//
// `docs/reference/std.md` is written from `compiler/semantic/src/stdlib_docs.rs`, and the same generator writes
// `docs/reference/std.segments.json`: the page cut into the Markdown that is the same in every language (the names,
// the signatures, the tables) and the English texts, each with a key and the hash of its text. A language translates the
// texts, not the page: `docs/reference/std.json` in its repository maps a key to the translated text and the hash of
// the English text it was translated from:
//
//   { "fn.std::text::trim": { "en": "<hash>", "text": "Quita los espacios de los dos extremos." } }
//
// The page of the language is the segments with the translated texts where the hash still matches, and the English
// text where the entry is missing or the English changed since (stale). The signatures always come from the English
// page, so they are never out of date.
//
// `node scripts/std-catalog.mjs <repository of the language> [--todo]` lists what is missing and what is stale;
// `--todo` prints them as entries to translate, with the current hash and the English text in `text`.

import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..');

/** The segments of the English page. */
export function readSegments(file = path.join(root, 'docs', 'reference', 'std.segments.json')) {
  return JSON.parse(fs.readFileSync(file, 'utf8'));
}

/**
 * The page in a language: its Markdown, how many distinct texts are translated out of how many, and the keys that are
 * missing, stale (the English changed) or unknown (in the catalog, not on the page anymore).
 */
export function renderReference(segments, catalog = {}) {
  let markdown = '';
  const keys = new Map();
  for (const s of segments) {
    if ('md' in s) {
      markdown += s.md;
      continue;
    }
    keys.set(s.key, s);
    const t = catalog[s.key];
    markdown += t && t.en === s.hash && typeof t.text === 'string' ? t.text : s.en;
  }
  const missing = [];
  const stale = [];
  let translated = 0;
  for (const [key, s] of keys) {
    const t = catalog[key];
    if (!t) missing.push(key);
    else if (t.en !== s.hash) stale.push(key);
    else translated++;
  }
  const unknown = Object.keys(catalog).filter((k) => !keys.has(k));
  return { markdown, translated, total: keys.size, missing, stale, unknown };
}

/** The entries a translator fills in: each missing or stale text, with its current hash and the English text. */
export function todo(segments, catalog = {}) {
  const { missing, stale } = renderReference(segments, catalog);
  const want = new Set([...missing, ...stale]);
  const out = {};
  for (const s of segments) {
    if ('key' in s && want.has(s.key) && !(s.key in out)) out[s.key] = { en: s.hash, text: s.en };
  }
  return out;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const [repo, flag] = process.argv.slice(2);
  if (!repo) {
    console.error('usage: node scripts/std-catalog.mjs <repository of the language> [--todo]');
    process.exit(2);
  }
  const file = path.join(repo, 'docs', 'reference', 'std.json');
  const catalog = fs.existsSync(file) ? JSON.parse(fs.readFileSync(file, 'utf8')) : {};
  const segments = readSegments();
  if (flag === '--todo') {
    console.log(JSON.stringify(todo(segments, catalog), null, 2));
  } else {
    const r = renderReference(segments, catalog);
    console.log(`${r.translated} of ${r.total} texts translated`);
    for (const k of r.stale) console.log(`stale (the English changed): ${k}`);
    for (const k of r.unknown) console.log(`not on the page anymore: ${k}`);
    console.log(`${r.missing.length} missing`);
    process.exit(r.stale.length || r.unknown.length ? 1 : 0);
  }
}
