# Documentation site

The site at <https://telvax.github.io/telvax/>, built with [Starlight](https://starlight.astro.build) (Astro).

The pages are not written here (the translated ones come from other repositories, see [Translations](#translations)). `npm run sync` copies `docs/guide/*.md`, `docs/reference/std.md`, `CHANGELOG.md`, `CONTRIBUTING.md`, `SECURITY.md`,
`CODE_OF_CONDUCT.md` and the two licenses into `src/content/docs/`, adds the title Starlight needs and rewrites the links (to other pages, or to the
repository on GitHub). Edit the files under `docs/`; the copies are ignored by git. Only `src/content/docs/index.mdx`,
the home page, is written here.

Code blocks marked `telvax` (or `tvx`) are highlighted with `editors/vscode/syntaxes/telvax.tmLanguage.json`,
the grammar of the VS Code extension.

The look follows the brand book: `src/styles/brand.css` has the palette (Carbono, Osso, Volt), the two type
families (Space Grotesk and JetBrains Mono, from `@fontsource-variable`) and the square corners; `src/code-theme.mjs`
is the theme of the code blocks; `src/assets/logo-*.svg` and `public/favicon.svg` are the symbol.

```sh
cd site
npm ci
npm run dev        # http://localhost:4321/telvax/
npm test           # the link rewriting
npm run build && node scripts/check-links.mjs   # what CI runs
```

`.github/workflows/site.yml` builds the site and checks its links on every pull request that touches the
documentation.

## Translations

The pages below are translated in other repositories of the organization: `telvax/docs-pt-br`,
`docs-es` and `docs-zh-cn`. The site shows them under `/pt-br/`, `/es/` and `/zh-cn/`, with the language picker of Starlight.
The home page of a language is the translation of `src/content/docs/index.mdx`, kept in the repository of the language as
`docs/site/index.mdx` and copied as it is, so it has the same hero, example and tables as the English one. (The README of
`telvax/telvax` is translated there too, but the site does not use it.) Each translated file starts with a line that says which
commit of this repository it comes from; the site takes it out of the page, so the pages look like the English ones, and keeps
it as the description of the page.

| Page on the site | File in the repository of the language |
|---|---|
| `guide/tour`, `guide/use-cases`, `guide/stability`, `guide/deploy`, `guide/compatibility` | `docs/guide/<name>.md` |
| `reference/std` (the library reference) | `docs/reference/std.json`, a catalog of texts (below) |
| `reference/cli` | `docs/reference/cli.md` |
| `whats-new` | `docs/about/whats-new.md`, the releases translated so far (below) |
| `about/contributing`, `about/security`, `about/code-of-conduct` | `docs/about/<name>.md` |
| the home page | `docs/site/index.mdx` |

The site takes the pages that a language has, one at a time (`localePages` in `scripts/convert.mjs`): a page that is not in
the repository is left out (with a line in the log), the language shows the English page with Starlight's notice, and the
links of a translated page go to the page of the language when it has it and to the English one when it does not.

**The library reference** is written from the code with every change of the standard library, so it is not translated as a
page. The generator also writes `docs/reference/std.segments.json`: the page cut into the Markdown that stays the same (the
names, the signatures, the tables) and the English texts, each with a key (`fn.std::text::trim`, `type.std::ml::Features`,
`method.Bytes.len`, `module.text`, `page.intro`, ...) and the hash of its text. A language translates texts, in any number:

```json
{ "fn.std::text::trim": { "en": "<hash of the English text>", "text": "Quita los espacios de los dos extremos." } }
```

The page of the language is the English page with the texts whose hash still matches. A text that is missing, or whose
English changed since (stale), shows in English, so the page is never wrong, only partly translated, and its description
says how much. `node scripts/std-catalog.mjs <repository of the language>` lists what is stale and how much is missing, and
fails when something is stale; with `--todo` it prints the entries to translate, with the current hash and the English
text in place of the translation.

**What's new** is translated a release at a time, when the release is published (its entries do not change after that),
newest first, from the current series on. `docs/about/whats-new.md` holds the releases translated so far under their
`## <version>` headings; the site adds a line in the language that links to the English page for the releases before the
oldest one there. `Unreleased` is not translated.

**The licenses** are not translated: only the English text is binding. Every language shows them in English, under a note in
the language that says so (`texts.license` in `scripts/locales.mjs`).

`npm run sync` reads the repositories from a folder, `TELVAX_TRANSLATIONS` or `site/.translations/` (git ignores it), as
`docs-pt-br/`, `docs-es/` and `docs-zh-cn/`. A language whose folder is not there is left out, so without them the site is
the English one. To see them while you write:

```sh
cd site
TELVAX_TRANSLATIONS=~/repos npm run dev      # a folder that holds the clones of the three repositories
```

The site workflow checks the three repositories out into `site/.translations/` and takes `main` of each as it is. A series
is built when it is released, so it keeps the translations of that day; `dev` is built every day as well, and a translation
merged in its repository is there within a day. To add a language, add it to `LOCALES` in `scripts/locales.mjs` (with the labels of the sidebar and its `texts`) and a
checkout step to the workflow.

## Versions

The site is published like the manual of other languages, one folder per version, on the `gh-pages` branch (a single
commit that the workflow rewrites):

| Address | What it is | Updated by |
|---|---|---|
| `/telvax/dev/` | the `main` branch, with a banner that says so | every push to `main` that changes the documentation |
| `/telvax/0.30/` | the series of a release (`v0.30.1` is in `0.30`) | the release workflow, for every release |
| `/telvax/` | goes to the newest series, or to `dev` while there is none | `scripts/publish.mjs` |

A link without a version, such as `/telvax/guide/tour/`, is sent to the newest series by `404.html`. The version
picker in the header reads `versions.json`. A series keeps the documentation of its last release; a release from
before `site/` existed has none. To publish by hand: Actions, Site, Run workflow, with `ref` (a branch or tag) and
`version` (`dev` or a series).

The environment variables `SITE_VERSION` and `SITE_BASE` (for example `0.30` and `/telvax/0.30`) make a build for
one folder; the workflow sets them. `SITE_URL` (and `SITE_BASE=/`) set the address if the site moves to its own domain.

There is no zip of the site to read offline: the pages link to each other with absolute addresses, so a copy opened
from a disk has no style and broken links.

To add a page to the site, add it to `PAGES` in `scripts/convert.mjs`.
