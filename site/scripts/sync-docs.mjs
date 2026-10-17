// Copies the documentation of the repository into the content folder of the site.
// The copies are build output (git ignores them); edit the files under `docs/`.

import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { PAGES, convert } from './convert.mjs';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..');
const content = path.join(root, 'site', 'src', 'content', 'docs');

/** When the file last changed, from git; nothing if git cannot say (no history, not a checkout). */
function lastChange(file) {
  try {
    const date = execFileSync('git', ['log', '-1', '--format=%cI', '--', file], {
      cwd: root,
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
