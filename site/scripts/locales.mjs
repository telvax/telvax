// The languages of the site besides English. Each one is a repository of the organization that holds a
// translation (docs/guide/tour.md, docs/guide/use-cases.md and docs/README.md, the home page of the language).
//
// The repositories are read from a folder: `TELVAX_TRANSLATIONS`, or `site/.translations/` (the site workflow
// checks them out there). A language whose folder is missing is left out, so a build without them is the
// English site.

import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const site = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

/**
 * `code` is the folder on the site (`/pt-br/`), `lang` the BCP 47 tag that Starlight uses for its own texts,
 * `repo` the repository (`url` is its address on GitHub). `sidebar` has the labels of the sidebar, by the key used in `astro.config.mjs`.
 */
export const LOCALES = [
  {
    code: 'pt-br',
    lang: 'pt-BR',
    label: 'Português (Brasil)',
    repo: 'docs-pt-br',
    sidebar: {
      tutorial: 'Tutorial',
      library: 'Referência da biblioteca',
      cli: 'Linha de comando',
      useCases: 'Como fazer: casos de uso',
      stability: 'Estabilidade da linguagem',
      whatsNew: 'Novidades',
      about: 'Sobre',
      contributing: 'Relatar problemas e contribuir',
      security: 'Segurança',
      conduct: 'Código de conduta',
      mit: 'Licença (MIT)',
      apache: 'Licença (Apache 2.0)',
    },
  },
  {
    code: 'es',
    lang: 'es',
    label: 'Español',
    repo: 'docs-es',
    sidebar: {
      tutorial: 'Tutorial',
      library: 'Referencia de la biblioteca',
      cli: 'Línea de comandos',
      useCases: 'Guías prácticas: casos de uso',
      stability: 'Estabilidad del lenguaje',
      whatsNew: 'Novedades',
      about: 'Acerca de',
      contributing: 'Informar de problemas y contribuir',
      security: 'Seguridad',
      conduct: 'Código de conducta',
      mit: 'Licencia (MIT)',
      apache: 'Licencia (Apache 2.0)',
    },
  },
  {
    code: 'zh-cn',
    lang: 'zh-CN',
    label: '简体中文',
    repo: 'docs-zh-cn',
    sidebar: {
      tutorial: '教程',
      library: '库参考',
      cli: '命令行',
      useCases: '操作指南：用例',
      stability: '语言稳定性',
      whatsNew: '新特性',
      about: '关于',
      contributing: '报告问题与参与贡献',
      security: '安全',
      conduct: '行为准则',
      mit: '许可证（MIT）',
      apache: '许可证（Apache 2.0）',
    },
  },
].map((l) => ({ ...l, url: `https://github.com/telvax/${l.repo}` }));

/** The folder that holds the repositories of the translations. */
export function translationsDir() {
  return path.resolve(process.env.TELVAX_TRANSLATIONS ?? path.join(site, '.translations'));
}

/** Where the repository of a language is, whether or not it is there. */
export function localeDir(locale) {
  return path.join(translationsDir(), locale.repo);
}

/** The languages whose repository is in the folder (it has the tour, the page every translation starts with). */
export function availableLocales() {
  return LOCALES.filter((l) => fs.existsSync(path.join(localeDir(l), 'docs', 'guide', 'tour.md')));
}
