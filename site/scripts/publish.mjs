// Puts a built version of the site into the folder that GitHub Pages serves (the `gh-pages` branch).
//
//   node scripts/publish.mjs --pages <folder> --dist <folder> --version dev|0.30 [--root /telvax]
//
// The folder holds one subfolder per version (`dev`, `0.30`, `0.29`, ...), a `versions.json` that the version
// picker reads, a root page that sends the reader to the newest release (or to `dev` while there is none) and a
// 404 page that sends links without a version, such as /telvax/guide/tour/, to the newest release.

import fs from 'node:fs';
import path from 'node:path';

/** `v0.30.1` and `0.30.1` are in the series `0.30`. */
export function seriesOf(tag) {
  const m = /^v?(\d+)\.(\d+)(?:\.\d+)?(?:[-+].*)?$/.exec(tag);
  if (!m) throw new Error(`"${tag}" is not a version such as v0.30.1`);
  return `${m[1]}.${m[2]}`;
}

export function isVersion(v) {
  return v === 'dev' || /^\d+\.\d+$/.test(v);
}

const order = (a, b) => {
  const [x, y] = [a, b].map((v) => v.split('.').map(Number));
  return y[0] - x[0] || y[1] - x[1];
};

/** The versions in the order of the picker: releases, newest first, then `dev`. */
export function upsert(list, version) {
  if (!isVersion(version)) throw new Error(`"${version}" is not "dev" or a series such as "0.30"`);
  const names = new Set(list.map((v) => v.version));
  names.add(version);
  const releases = [...names].filter((v) => v !== 'dev').sort(order);
  const all = [...releases, ...(names.has('dev') ? ['dev'] : [])];
  return all.map((v) => ({
    version: v,
    label: v === 'dev' ? 'dev (main)' : v === releases[0] ? `${v} (latest)` : v,
    latest: v === releases[0],
  }));
}

/** The version a reader is sent to: the newest release, or `dev` while there is none. */
export function landing(list) {
  return list.find((v) => v.latest)?.version ?? 'dev';
}

export function rootPage(root, version) {
  const to = `${root}/${version}/`;
  return `<!doctype html>
<meta charset="utf-8">
<title>Telvax documentation</title>
<meta http-equiv="refresh" content="0; url=${to}">
<link rel="canonical" href="${to}">
<p>Redirecting to <a href="${to}">the documentation</a>.</p>
`;
}

/** A link without a version goes to the newest release; a version that has no such page is a plain 404. */
export function notFoundPage(root) {
  return `<!doctype html>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Page not found</title>
<body style="font-family: system-ui, sans-serif; max-width: 40rem; margin: 4rem auto; padding: 0 1rem">
<h1>Page not found</h1>
<p>There is no such page. <a href="${root}/">Go to the documentation.</a></p>
<script>
  (async () => {
    const root = ${JSON.stringify(root)};
    const rest = location.pathname.startsWith(root + '/') ? location.pathname.slice(root.length + 1) : '';
    const first = rest.split('/')[0];
    try {
      const versions = await (await fetch(root + '/versions.json')).json();
      if (!rest || versions.some((v) => v.version === first)) return;
      const latest = (versions.find((v) => v.latest) || versions[0]).version;
      location.replace(root + '/' + latest + '/' + rest + location.search + location.hash);
    } catch {}
  })();
</script>
`;
}

function main(argv) {
  const arg = (name) => {
    const i = argv.indexOf(name);
    return i === -1 ? undefined : argv[i + 1];
  };
  const [pages, dist, version] = [arg('--pages'), arg('--dist'), arg('--version')];
  const root = (arg('--root') ?? '/telvax').replace(/\/$/, '');
  if (!pages || !dist || !version) {
    console.error('usage: publish.mjs --pages <folder> --dist <folder> --version dev|0.30 [--root /telvax]');
    process.exit(2);
  }
  const target = path.join(pages, version);
  upsert([], version); // checks the name before anything is deleted
  fs.rmSync(target, { recursive: true, force: true });
  fs.cpSync(dist, target, { recursive: true });

  const file = path.join(pages, 'versions.json');
  const known = fs.existsSync(file) ? JSON.parse(fs.readFileSync(file, 'utf8')) : [];
  const list = upsert(known, version);
  fs.writeFileSync(file, JSON.stringify(list, null, 2) + '\n');
  fs.writeFileSync(path.join(pages, 'index.html'), rootPage(root, landing(list)));
  fs.writeFileSync(path.join(pages, '404.html'), notFoundPage(root));
  // Pages would otherwise skip folders that start with an underscore (`_astro`).
  fs.writeFileSync(path.join(pages, '.nojekyll'), '');
  console.log(`${version} published; versions: ${list.map((v) => v.version).join(', ')}; the root goes to ${landing(list)}`);
}

if (process.argv[1] && path.resolve(process.argv[1]) === new URL(import.meta.url).pathname) main(process.argv.slice(2));
