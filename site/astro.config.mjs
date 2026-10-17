import fs from 'node:fs';
import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';

// Code blocks tagged `telvax` (or `tvx`) use the same TextMate grammar as the VS Code extension.
const grammar = JSON.parse(
  fs.readFileSync(new URL('../editors/vscode/syntaxes/telvax.tmLanguage.json', import.meta.url), 'utf8'),
);

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
        { slug: 'guide/tour', label: 'Tutorial' },
        { slug: 'reference/std', label: 'Library reference' },
        { slug: 'reference/cli', label: 'Command line' },
        { slug: 'guide/use-cases', label: 'HOWTOs: use cases' },
        { slug: 'guide/stability', label: 'Language stability' },
        { slug: 'whats-new', label: "What's new" },
        {
          label: 'About',
          items: [
            { slug: 'about/contributing', label: 'Reporting problems and contributing' },
            { slug: 'about/security', label: 'Security' },
            { slug: 'about/code-of-conduct', label: 'Code of conduct' },
            { slug: 'about/license-mit', label: 'License (MIT)' },
            { slug: 'about/license-apache', label: 'License (Apache 2.0)' },
          ],
        },
      ],
    }),
  ],
});
