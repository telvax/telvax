import fs from 'node:fs';
import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';
import { availableLocales } from './scripts/locales.mjs';

// Code blocks tagged `telvax` (or `tvx`) use the same TextMate grammar as the VS Code extension.
const grammar = JSON.parse(
  fs.readFileSync(new URL('../editors/vscode/syntaxes/telvax.tmLanguage.json', import.meta.url), 'utf8'),
);

// The languages with a translation in the folder of `scripts/locales.mjs`. With none, the site is English only.
const locales = availableLocales();

/** A sidebar entry with its label in English and in each language of the site. */
const entry = (key, label, item) => ({
  ...item,
  label,
  translations: Object.fromEntries(locales.map((l) => [l.lang, l.sidebar[key]])),
});

export default defineConfig({
  // GitHub Pages serves a repository at https://<org>.github.io/<repo>/. A custom domain sets
  // SITE_URL and SITE_BASE=/ in the workflow. A published version sits in its own folder: SITE_BASE=/telvax/0.30
  // and SITE_VERSION=0.30 (`dev` for the main branch).
  site: process.env.SITE_URL ?? 'https://telvax.github.io',
  base: process.env.SITE_BASE ?? '/telvax',
  integrations: [
    starlight({
      title: 'Telvax',
      description: 'A statically typed, compiled language for data, ML pipelines and AI serving.',
      social: [{ icon: 'github', label: 'GitHub', href: 'https://github.com/telvax/telvax' }],
      lastUpdated: true,
      // English is the root of the site, each translation a folder (`/pt-br/`). A page that is not translated
      // shows the English one, with a notice.
      ...(locales.length && {
        defaultLocale: 'root',
        locales: {
          root: { label: 'English', lang: 'en' },
          ...Object.fromEntries(locales.map((l) => [l.code, { label: l.label, lang: l.lang }])),
        },
      }),
      components: {
        ThemeSelect: './src/components/ThemeSelect.astro',
        Banner: './src/components/Banner.astro',
      },
      editLink: { baseUrl: 'https://github.com/telvax/telvax/edit/main/' },
      expressiveCode: {
        shiki: {
          langs: [{ ...grammar, name: 'telvax', aliases: ['tvx'] }],
        },
      },
      sidebar: [
        // The sections of a language manual: tutorial, library reference, how-tos, language rules.
        entry('tutorial', 'Tutorial', { slug: 'guide/tour' }),
        entry('library', 'Library reference', { slug: 'reference/std' }),
        entry('cli', 'Command line', { slug: 'reference/cli' }),
        entry('useCases', 'HOWTOs: use cases', { slug: 'guide/use-cases' }),
        entry('stability', 'Language stability', { slug: 'guide/stability' }),
        entry('whatsNew', "What's new", { slug: 'whats-new' }),
        entry('about', 'About', {
          items: [
            entry('contributing', 'Reporting problems and contributing', { slug: 'about/contributing' }),
            entry('security', 'Security', { slug: 'about/security' }),
            entry('conduct', 'Code of conduct', { slug: 'about/code-of-conduct' }),
            entry('mit', 'License (MIT)', { slug: 'about/license-mit' }),
            entry('apache', 'License (Apache 2.0)', { slug: 'about/license-apache' }),
          ],
        }),
      ],
    }),
  ],
});
