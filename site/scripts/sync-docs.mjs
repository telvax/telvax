// Copies the documentation of the repository into the content folder of the site, and the pages of the
// translations that are in the folder of `locales.mjs`.
// The copies are build output (git ignores them); edit the files under `docs/`, or in the repository of the language.

import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { PAGES, convert, localePages } from './convert.mjs';
import { LOCALES, availableLocales, localeDir } from './locales.mjs';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..');
const content = path.join(root, 'site', 'src', 'content', 'docs');

/** When the file last changed, from git; nothing if git cannot say (no history, not a checkout). */
function lastChange(file, cwd = root) {
  try {
    const date = execFileSync('git', ['log', '-1', '--format=%cI', '--', file], {
      cwd,
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'ignore'],
    }).trim();
    return date || null;
  } catch {
    return null;
  }
}

for (const page of PAGES) {
  const text = fs.readFileSync(path.join(root, page.src), 'utf8');
  const out = path.join(content, `${page.out}.md`);
  fs.mkdirSync(path.dirname(out), { recursive: true });
  fs.writeFileSync(out, convert(page, text, PAGES, lastChange(page.src)));
  console.log(`${page.src} -> site/src/content/docs/${page.out}.md`);
}

// A language that is not in the folder anymore must not keep the pages of an earlier sync.
for (const locale of LOCALES) fs.rmSync(path.join(content, locale.code), { recursive: true, force: true });

const available = availableLocales();
for (const locale of available) {
  const dir = localeDir(locale);
  const pages = localePages(locale);
  for (const page of pages) {
    const file = path.join(dir, page.src);
    if (!fs.existsSync(file)) {
      console.warn(`${locale.repo}/${page.src} is not there; the page is left out`);
      continue;
    }
    const text = fs.readFileSync(file, 'utf8');
    const ext = page.raw ? path.extname(page.src) : '.md';
    const out = path.join(content, `${page.out}${ext}`);
    fs.mkdirSync(path.dirname(out), { recursive: true });
    fs.writeFileSync(out, page.raw ? text : convert(page, text, pages, lastChange(page.src, dir), locale));
    console.log(`${locale.repo}/${page.src} -> site/src/content/docs/${page.out}${ext}`);
  }
}
for (const locale of LOCALES.filter((l) => !available.includes(l))) {
  console.log(`no translation in ${localeDir(locale)}: /${locale.code}/ is not built`);
}
