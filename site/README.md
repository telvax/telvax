# Documentation site

The site at <https://telvax.github.io/telvax/>, built with [Starlight](https://starlight.astro.build) (Astro).

The pages are not written here (the translated ones come from other repositories, see [Translations](#translations)). `npm run sync` copies `docs/guide/*.md`, `docs/reference/std.md`, `CHANGELOG.md`, `CONTRIBUTING.md`, `SECURITY.md`,
`CODE_OF_CONDUCT.md` and the two licenses into `src/content/docs/`, adds the title Starlight needs and rewrites the links (to other pages, or to the
repository on GitHub). Edit the files under `docs/`; the copies are ignored by git. Only `src/content/docs/index.mdx`,
the home page, is written here.

Code blocks marked `telvax` (or `tvx`) are highlighted with `editors/vscode/syntaxes/telvax.tmLanguage.json`,
the grammar of the VS Code extension.

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

The tour, the use cases and the README are translated in other repositories of the organization: `telvax/docs-pt-br`,
`docs-es` and `docs-zh-cn`. The site shows them under `/pt-br/`, `/es/` and `/zh-cn/`, with the language picker of Starlight.
The README of a language is its home page. The other pages (the library reference, the command line, stability, what's new
and the pages about the project) are not translated: the language shows the English page with Starlight's notice, and
the links of a translated page to those pages go to the English ones.

`npm run sync` reads the repositories from a folder, `TELVAX_TRANSLATIONS` or `site/.translations/` (git ignores it), as
`docs-pt-br/`, `docs-es/` and `docs-zh-cn/`. A language whose folder is not there is left out, so without them the site is
the English one. To see them while you write:

```sh
cd site
TELVAX_TRANSLATIONS=~/repos npm run dev      # a folder that holds the clones of the three repositories
```

The site workflow checks the three repositories out into `site/.translations/` and takes `main` of each as it is. A series
is built when it is released, so it keeps the translations of that day; `dev` is built every day as well, and a translation
merged in its repository is there within a day. Each translated page starts with the commit of this repository it was
translated from. To add a language, add it to `LOCALES` in `scripts/locales.mjs` (with the labels of the sidebar) and a
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
