import assert from 'node:assert/strict';
import test from 'node:test';
import { PAGES, convert, localePages, rewriteLink, takeSourceNote } from './convert.mjs';

const tour = PAGES.find((p) => p.src === 'docs/guide/tour.md');
const std = PAGES.find((p) => p.src === 'docs/reference/std.md');

test('links between pages become relative site links', () => {
  assert.equal(rewriteLink('use-cases.md', tour.src), '../use-cases/');
  assert.equal(rewriteLink('tour.md#distribution', 'docs/guide/use-cases.md'), '../tour/#distribution');
  assert.equal(rewriteLink('../reference/std.md#text', tour.src), '../../reference/std/#text');
  assert.equal(rewriteLink('../guide/tour.md#x', std.src), '../../guide/tour/#x');
});

test('links to the repository point at GitHub', () => {
  const repo = 'https://github.com/telvax/telvax';
  assert.equal(rewriteLink('../../examples/ml/forward.tvx', tour.src), `${repo}/blob/main/examples/ml/forward.tvx`);
  assert.equal(rewriteLink('../../examples', tour.src), `${repo}/tree/main/examples`);
  assert.equal(rewriteLink('../design/status.md', tour.src), `${repo}/blob/main/docs/design/status.md`);
  assert.equal(rewriteLink('../../README.md#language-notes', tour.src), `${repo}/blob/main/README.md#language-notes`);
});

test('external and anchor links stay', () => {
  assert.equal(rewriteLink('https://rustup.rs', tour.src), 'https://rustup.rs');
  assert.equal(rewriteLink('#modules', tour.src), '#modules');
});

test('the title moves to the front matter and code blocks are left alone', () => {
  const md = '# A tour\n\nSee [cases](use-cases.md).\n\n```telvax\nlet x = a[b](c.md)\n```\n';
  const out = convert(tour, md);
  assert.match(out, /^---\ntitle: "A tour"\n/);
  assert.match(out, /editUrl: https:\/\/github.com\/telvax\/telvax\/edit\/main\/docs\/guide\/tour.md/);
  assert.ok(!out.includes('# A tour'));
  assert.ok(out.includes('[cases](../use-cases/)'));
  assert.ok(out.includes('a[b](c.md)'), 'code is not rewritten');
});

test('inline code that looks like a link is left alone', () => {
  const out = convert(tour, '# T\n\nUse `f](self, g)` here and `x](1, "a")`.\n');
  assert.ok(out.includes('`f](self, g)`'));
});

test('a page without a title is an error', () => {
  assert.throws(() => convert(tour, 'no title'), /no "# Title"/);
});

test('a text page is shown as it is, under the title given', () => {
  const mit = PAGES.find((p) => p.src === 'LICENSE-MIT');
  const out = convert(mit, 'MIT License\n\nCopyright (c) 2026 [x](y.md)\n');
  assert.match(out, /title: "MIT License"/);
  assert.ok(out.includes('```text\nMIT License'), 'the text is in a code block');
  assert.ok(out.includes('[x](y.md)'), 'links in a license are not rewritten');
  assert.throws(() => convert({ src: 'X', text: true }, 'x'), /needs a title/);
});

test('the title of a page can be replaced and the date of its change is kept', () => {
  const changelog = PAGES.find((p) => p.src === 'CHANGELOG.md');
  const out = convert(changelog, '# Changelog\n\n## 0.1.0\n', PAGES, '2026-10-17T10:00:00-03:00');
  assert.match(out, /title: "What's new"/);
  assert.match(out, /lastUpdated: 2026-10-17T10:00:00-03:00/);
  assert.ok(!out.includes('# Changelog'));
});

// ---- translations ----

const pt = {
  code: 'pt-br',
  lang: 'pt-BR',
  repo: 'docs-pt-br',
  url: 'https://github.com/telvax/docs-pt-br',
};
const ptPages = localePages(pt);
const ptTour = ptPages.find((p) => p.src === 'docs/guide/tour.md');
// A page that is the folder of its language (`pt-br/index`), to try the links to and from it. The home page of the
// site is copied as it is, so it is not converted; the rule of the links is the same.
const homePages = [...ptPages.filter((p) => !p.raw), { src: 'docs/README.md', out: 'pt-br/index' }];
const ptHome = homePages.at(-1);

test('the pages of a language are the tour, the use cases and the home page, which is copied as it is', () => {
  assert.deepEqual(
    ptPages.map((p) => p.out),
    ['pt-br/guide/tour', 'pt-br/guide/use-cases', 'pt-br/index'],
  );
  assert.deepEqual(
    ptPages.map((p) => !!p.raw),
    [false, false, true],
  );
  assert.equal(ptPages.at(-1).src, 'docs/site/index.mdx');
});

test('links between translated pages stay in the language', () => {
  assert.equal(rewriteLink('use-cases.md', ptTour.src, ptPages, pt), '../use-cases/');
  assert.equal(rewriteLink('tour.md#módulos-e-pacotes', 'docs/guide/use-cases.md', ptPages, pt), '../tour/#módulos-e-pacotes');
  // The home page is the folder of the language: its links go down, and a link to it goes up.
  assert.equal(rewriteLink('guide/tour.md', ptHome.src, homePages, pt), 'guide/tour/');
  assert.equal(rewriteLink('../README.md', ptTour.src, homePages, pt), '../../');
});

test('a link to a page of the English site goes to that page, in the same version', () => {
  const gh = 'https://github.com/telvax/telvax/blob/main';
  assert.equal(rewriteLink(`${gh}/docs/reference/std.md#text`, ptTour.src, ptPages, pt), '../../../reference/std/#text');
  // The home page of the language is the folder `/pt-br/`, so the English pages are one level up.
  assert.equal(rewriteLink(`${gh}/docs/reference/cli.md`, ptHome.src, homePages, pt), '../reference/cli/');
  assert.equal(rewriteLink(`${gh}/CONTRIBUTING.md`, ptHome.src, homePages, pt), '../about/contributing/');
});

test('a link to the repository that is not a page of the site stays on GitHub', () => {
  const gh = 'https://github.com/telvax/telvax/blob/main';
  assert.equal(rewriteLink(`${gh}/examples/ml/forward.tvx`, ptTour.src, ptPages, pt), `${gh}/examples/ml/forward.tvx`);
  assert.equal(rewriteLink(`${gh}/docs/design/status.md`, ptHome.src, homePages, pt), `${gh}/docs/design/status.md`);
  assert.equal(rewriteLink('https://rustup.rs', ptTour.src, ptPages, pt), 'https://rustup.rs');
  // The English site does not read links to GitHub as pages of itself.
  assert.equal(rewriteLink(`${gh}/docs/reference/std.md`, tour.src), `${gh}/docs/reference/std.md`);
  // Anything relative that is not a page is in the repository of the language.
  assert.equal(rewriteLink('../SECURITY.md', ptHome.src, homePages, pt), 'https://github.com/telvax/docs-pt-br/blob/main/SECURITY.md');
});

test('a translated page edits in its own repository and the home page is not in the sidebar order', () => {
  const md = '> Traduzido de [telvax/telvax@1c88355](https://x)\n\n# Um tour\n\nVeja [casos](use-cases.md).\n';
  const out = convert(ptTour, md, ptPages, null, pt);
  assert.match(out, /^---\ntitle: "Um tour"\n/);
  assert.match(out, /editUrl: https:\/\/github.com\/telvax\/docs-pt-br\/edit\/main\/docs\/guide\/tour.md/);
  assert.match(out, /sidebar:\n {2}order: 1/);
  assert.ok(out.includes('[casos](../use-cases/)'));
  const home = convert(ptHome, '<div>\n\n# Telvax\n\n</div>\n', homePages, null, pt);
  assert.match(home, /title: "Telvax"/);
  assert.ok(!home.includes('sidebar:'), 'the home page has no order');
});

test('the line with the commit a translation comes from is taken out of the page and kept as its description', () => {
  const md = '> Traduzido de [telvax/telvax@1c88355](https://github.com/telvax/telvax/commit/1c88355) (`docs/guide/tour.md`).\n\n# Um tour\n\nTexto.\n';
  const out = convert(ptTour, md, ptPages, null, pt);
  assert.ok(!out.includes('Traduzido de ['), 'not in the body');
  assert.match(out, /description: "Traduzido de telvax\/telvax@1c88355 \(`docs\/guide\/tour.md`\)\."/);
  assert.ok(out.includes('\nTexto.\n'));
  assert.ok(!/\n> /.test(out));
  // The English pages have no such line, and a quotation that is not it stays.
  assert.ok(!convert(tour, '# T\n\ntext\n').includes('description:'));
  const quote = convert(ptTour, '# Um tour\n\n> Uma citação.\n', ptPages, null, pt);
  assert.ok(quote.includes('> Uma citação.') && !quote.includes('description:'));
});

test('takeSourceNote reads a quotation of several lines and leaves the rest', () => {
  const lines = ['', '> 译自 [telvax/telvax@1c88355](https://x)（`docs/guide/tour.md`）。', '> Segunda linha.', '', '# 标题'];
  assert.equal(takeSourceNote(lines), '译自 telvax/telvax@1c88355（`docs/guide/tour.md`）。 Segunda linha.');
  assert.deepEqual(lines, ['', '', '# 标题']);
  assert.equal(takeSourceNote(['# Title']), null);
  assert.equal(takeSourceNote(['', '']), null);
});
