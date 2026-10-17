import assert from 'node:assert/strict';
import test from 'node:test';
import { PAGES, convert, rewriteLink } from './convert.mjs';

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
