// Turns a Markdown file of the repository into a page of the site.
//
// The pages stay where they are (`docs/guide`, `docs/reference`) so that a change and its
// documentation are one pull request. This script only adds what Starlight needs: a front matter with the
// title, and links that work on the site.

import path from 'node:path';
import { fill } from './locales.mjs';

const REPO = 'https://github.com/telvax/telvax';

/**
 * Source file (from the repository root) -> where it is on the site. `title` replaces the first heading;
 * `text: true` is a plain text file (a license) shown as it is.
 */
export const PAGES = [
  { src: 'docs/guide/tour.md', out: 'guide/tour', order: 1 },
  { src: 'docs/guide/use-cases.md', out: 'guide/use-cases', order: 2 },
  { src: 'docs/guide/stability.md', out: 'guide/stability', order: 3 },
  { src: 'docs/guide/deploy.md', out: 'guide/deploy', order: 4 },
  { src: 'docs/guide/compatibility.md', out: 'guide/compatibility', order: 5 },
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
 * is the translation of `site/src/content/docs/index.mdx`: it is copied as it is (`raw`), because it is already a page
 * of the site, with the hero and the links relative to the pages.
 *
 * The list is every page that a language can translate, with the path that mirrors the English one. A page that is
 * not in the repository of the language is left out by `sync-docs.mjs`, and the English page shows in its place.
 *
 * Two are not translated as whole pages:
 * * the library reference (`reference`) is the English page with the texts of a catalog, `docs/reference/std.json`
 *   (see `std-catalog.mjs`);
 * * what's new (`whatsNew`) holds the releases translated so far, newest first, and ends with a link to the English page
 *   for the older ones (see `whatsNewEnd`).
 *
 * The licenses are not translated: only the English text is binding. Every language gets them in English, under a
 * note in the language that says so (`licensePage`).
 */
export function localePages(locale) {
  return [
    { src: 'docs/guide/tour.md', out: `${locale.code}/guide/tour`, order: 1 },
    { src: 'docs/guide/use-cases.md', out: `${locale.code}/guide/use-cases`, order: 2 },
    { src: 'docs/guide/stability.md', out: `${locale.code}/guide/stability`, order: 3 },
    { src: 'docs/guide/deploy.md', out: `${locale.code}/guide/deploy`, order: 4 },
    { src: 'docs/guide/compatibility.md', out: `${locale.code}/guide/compatibility`, order: 5 },
    { src: 'docs/reference/std.json', out: `${locale.code}/reference/std`, order: 1, reference: true },
    { src: 'docs/reference/cli.md', out: `${locale.code}/reference/cli`, order: 2 },
    { src: 'docs/about/whats-new.md', out: `${locale.code}/whats-new`, order: 1, whatsNew: true },
    { src: 'docs/about/contributing.md', out: `${locale.code}/about/contributing`, order: 1 },
    { src: 'docs/about/security.md', out: `${locale.code}/about/security`, order: 2 },
    { src: 'docs/about/code-of-conduct.md', out: `${locale.code}/about/code-of-conduct`, order: 3 },
    { src: 'docs/site/index.mdx', out: `${locale.code}/index`, raw: true },
  ];
}

/**
 * Every translated file starts with a line that says which commit of this repository it was translated from
 * (`> Traducido de [telvax/telvax@1c88355](...) (docs/guide/tour.md).`). The English pages do not have it, so the
 * line is taken out of the page and kept as its description. Gives the text of the line, or null if there is none.
 */
export function takeSourceNote(lines) {
  const first = lines.findIndex((l) => l.trim() !== '');
  if (first === -1 || !/^>\s*.*telvax\/telvax@[0-9a-f]{7,}/.test(lines[first])) return null;
  let end = first;
  while (end < lines.length && lines[end].startsWith('>')) end++;
  const text = lines
    .slice(first, end)
    .map((l) => l.replace(/^>\s*/, ''))
    .join(' ')
    .replace(/\[([^\]]+)\]\([^)]*\)/g, '$1')
    .trim();
  lines.splice(first, end - first);
  return text;
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
    // The page of the language when it has it (it is at the same path under its code), the English page when not.
    const english = PAGES.find((p) => p.src === file);
    const own = english && pages.find((p) => p.out === `${locale.code}/${english.out}`);
    if (english) return siteLink(here.out, (own ?? english).out, hash ? `#${hash}` : '');
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
  const note = locale ? takeSourceNote(lines) : null;
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
    ...(note ? [`description: ${JSON.stringify(note)}`] : []),
    `editUrl: ${locale ? locale.url : REPO}/edit/main/${page.src}`,
    ...(page.order === undefined ? [] : [`sidebar:\n  order: ${page.order}`]),
    ...(lastUpdated ? [`lastUpdated: ${lastUpdated}`] : []),
    '---',
    '',
  ].join('\n');
  return front + body;
}

/**
 * The end of a translated what's new: the oldest release it has (the last `## <version>` heading) and a line in the
 * language that links to the English page for the releases before it. Nothing when it has no release yet.
 */
export function whatsNewEnd(text, locale, out) {
  const versions = [...text.matchAll(/^##\s+\[?v?(\d+\.\d+\.\d+[^\]\s]*)/gm)].map((m) => m[1]);
  if (versions.length === 0) return '';
  const link = `${path.posix.relative(urlDir(out), 'whats-new')}/`;
  return `\n\n${fill(locale.texts.olderReleases, { version: versions.at(-1), link })}\n`;
}

/** A license in a language: the English text as it is, under a note in the language that only it is binding. */
export function licensePage(page, text, locale) {
  const label = page.out.endsWith('mit') ? locale.sidebar.mit : locale.sidebar.apache;
  const own = { ...page, out: `${locale.code}/${page.out}`, title: label };
  const converted = convert(own, text, PAGES);
  const end = converted.indexOf('\n---\n', 4) + 5;
  return `${converted.slice(0, end)}\n:::note\n${locale.texts.license}\n:::\n\n${converted.slice(end)}`;
}

