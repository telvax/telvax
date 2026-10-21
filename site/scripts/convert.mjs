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

/**
 * The pages of a translation (see `locales.mjs`), in the repository of the language. The home page of the language
 * is its README, the translation of the README of this repository.
 */
export function localePages(locale) {
  return [
    { src: 'docs/guide/tour.md', out: `${locale.code}/guide/tour`, order: 1 },
    { src: 'docs/guide/use-cases.md', out: `${locale.code}/guide/use-cases`, order: 2 },
    { src: 'docs/README.md', out: `${locale.code}/index` },
  ];
}

/** The address of a page, as a folder: `guide/tour` is `guide/tour/`, and the home page of `es` is `es/`. */
function urlDir(out) {
  return out.replace(/(^|\/)index$/, '');
}

/** A link from the page `from` to the page `to` (both `out` paths), keeping the `#anchor`. */
function siteLink(from, to, anchor) {
  const rel = path.posix.relative(urlDir(from), urlDir(to)) || '.';
  return `${rel}/${anchor}`;
}

/**
 * Rewrites one link target found in `from` (a path in the repository). `locale` is given for the page of a
 * translation: its other links are to the repository of the language, and a link to a page of the English site
 * (the library reference, the command line) goes to that page and not to GitHub.
 */
export function rewriteLink(target, from, pages = PAGES, locale = null) {
  const here = pages.find((p) => p.src === from);
  if (locale && target.startsWith(`${REPO}/blob/main/`)) {
    const [file, hash = ''] = target.slice(`${REPO}/blob/main/`.length).split('#');
    const english = PAGES.find((p) => p.src === file);
    if (english) return siteLink(here.out, english.out, hash ? `#${hash}` : '');
  }
  if (/^([a-z][a-z0-9+.-]*:|#|\/\/)/i.test(target)) return target;
  const [file, hash = ''] = target.split('#');
  const anchor = hash ? `#${hash}` : '';
  const resolved = path.posix.normalize(path.posix.join(path.posix.dirname(from), file));
  const page = pages.find((p) => p.src === resolved);
  if (page) return siteLink(here.out, page.out, anchor);
  // Anything else is in the repository: files open in the browser, folders list their content.
  const kind = path.posix.extname(resolved) ? 'blob' : 'tree';
  return `${locale ? locale.url : REPO}/${kind}/main/${resolved}${anchor}`;
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

/**
 * The site page for `page.src`, given the file's text and, if known, the date of its last change. `locale` is
 * given for the page of a translation, with `pages` the ones of `localePages`.
 */
export function convert(page, text, pages = PAGES, lastUpdated = null, locale = null) {
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
    line.replace(/\]\(([^)\s,]+)\)/g, (_, t) => `](${rewriteLink(t, page.src, pages, locale)})`);
  const body = (page.text ? lines.join('\n') : outsideFences(lines.join('\n'), rewrite)).replace(/^\n+/, '');
  const front = [
    '---',
    `title: ${JSON.stringify(title)}`,
    `editUrl: ${locale ? locale.url : REPO}/edit/main/${page.src}`,
    ...(page.order === undefined ? [] : [`sidebar:\n  order: ${page.order}`]),
    ...(lastUpdated ? [`lastUpdated: ${lastUpdated}`] : []),
    '---',
    '',
  ].join('\n');
  return front + body;
}
