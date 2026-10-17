// Turns a Markdown file of the repository into a page of the site.
//
// The pages stay where they are (`docs/guide`, `docs/reference`) so that a change and its
// documentation are one pull request. This script only adds what Starlight needs: a front matter with the
// title, and links that work on the site.

import path from 'node:path';

const REPO = 'https://github.com/telvax/telvax';

/**
 * Source file (from the repository root) -> where it is on the site. `title` replaces the first heading;
 * `text: true` is a plain text file (a license) shown as it is.
 */
export const PAGES = [
  { src: 'docs/guide/tour.md', out: 'guide/tour', order: 1 },
  { src: 'docs/guide/use-cases.md', out: 'guide/use-cases', order: 2 },
  { src: 'docs/guide/stability.md', out: 'guide/stability', order: 3 },
  { src: 'docs/reference/std.md', out: 'reference/std', order: 1 },
  { src: 'docs/reference/cli.md', out: 'reference/cli', order: 2 },
  { src: 'CHANGELOG.md', out: 'whats-new', order: 1, title: "What's new" },
  { src: 'CONTRIBUTING.md', out: 'about/contributing', order: 1, title: 'Reporting problems and contributing' },
  { src: 'SECURITY.md', out: 'about/security', order: 2 },
  { src: 'CODE_OF_CONDUCT.md', out: 'about/code-of-conduct', order: 3 },
  { src: 'LICENSE-MIT', out: 'about/license-mit', order: 4, title: 'MIT License', text: true },
  { src: 'LICENSE-APACHE', out: 'about/license-apache', order: 5, title: 'Apache License 2.0', text: true },
];

/** Rewrites one link target found in `from` (a path in the repository). */
export function rewriteLink(target, from, pages = PAGES) {
  if (/^([a-z][a-z0-9+.-]*:|#|\/\/)/i.test(target)) return target;
  const [file, hash = ''] = target.split('#');
  const anchor = hash ? `#${hash}` : '';
  const resolved = path.posix.normalize(path.posix.join(path.posix.dirname(from), file));
  const page = pages.find((p) => p.src === resolved);
  if (page) {
    const here = pages.find((p) => p.src === from);
    const rel = path.posix.relative(here.out, page.out) || '.';
    return `${rel}/${anchor}`;
  }
  // Anything else is in the repository: files open in the browser, folders list their content.
  const kind = path.posix.extname(resolved) ? 'blob' : 'tree';
  return `${REPO}/${kind}/main/${resolved}${anchor}`;
}

/** Applies `fn` to the text outside fenced code blocks. */
function outsideFences(text, fn) {
  let fence = null;
  return text
    .split('\n')
    .map((line) => {
      const m = line.match(/^\s*(`{3,}|~{3,})/);
      if (m) {
        if (!fence) fence = m[1][0];
        else if (m[1][0] === fence) fence = null;
        return line;
      }
      return fence ? line : fn(line);
    })
    .join('\n');
}

/** The site page for `page.src`, given the file's text and, if known, the date of its last change. */
export function convert(page, text, pages = PAGES, lastUpdated = null) {
  const lines = text.replace(/\r\n/g, '\n').split('\n');
  let title = page.title;
  if (page.text) {
    if (!title) throw new Error(`${page.src}: a text page needs a title`);
    lines.splice(0, lines.length, '```text', ...text.replace(/\s+$/, '').split('\n'), '```');
  } else {
    const h1 = lines.findIndex((l) => /^# /.test(l));
    if (h1 === -1) throw new Error(`${page.src}: no "# Title" line`);
    title ??= lines[h1].slice(2).trim();
    lines.splice(h1, 1);
  }
  // Links like `](other.md)`; a target with a space or a comma is code such as `](self, f)`.
  const rewrite = (line) =>
    line.replace(/\]\(([^)\s,]+)\)/g, (_, t) => `](${rewriteLink(t, page.src, pages)})`);
  const body = (page.text ? lines.join('\n') : outsideFences(lines.join('\n'), rewrite)).replace(/^\n+/, '');
  const front = [
    '---',
    `title: ${JSON.stringify(title)}`,
    `editUrl: ${REPO}/edit/main/${page.src}`,
    `sidebar:\n  order: ${page.order}`,
    ...(lastUpdated ? [`lastUpdated: ${lastUpdated}`] : []),
    '---',
    '',
  ].join('\n');
  return front + body;
}
