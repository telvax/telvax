# Documentation site

The site at <https://telvax.github.io/telvax/>, built with [Starlight](https://starlight.astro.build) (Astro).

The pages are not written here. `npm run sync` copies `docs/guide/*.md`, `docs/reference/std.md`, `CHANGELOG.md`, `CONTRIBUTING.md`, `SECURITY.md`,
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
