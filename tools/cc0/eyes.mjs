// Fetches candidate photos for the open or closed eyes test (best shots):
// CC0 and public domain portraits from Openverse, into
// fixtures-out/eyes/candidates/ with their licenses in candidates.json.
// Which ones show closed eyes is decided by looking at them; the labels
// live in tools/cc0/eyes.json (keyed by source URL).
//
//   node tools/cc0/eyes.mjs [query…]   (more queries add to the candidates)

import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const out = join(root, 'fixtures-out', 'eyes', 'candidates');
const UA = 'roll-fixtures/0.1 (https://github.com/mrghxst/roll; test set builder)';
const DEFAULT = ['eyes closed portrait', 'closed eyes', 'sleeping baby', 'sleeping man', 'woman eyes closed', 'blink', 'portrait face', 'smiling woman portrait', 'man portrait face', 'child portrait'];
const QUERIES = process.argv.length > 2 ? process.argv.slice(2) : DEFAULT;
const PER = 12;

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

mkdirSync(out, { recursive: true });
const index = join(out, 'candidates.json');
const list = existsSync(index) ? JSON.parse(readFileSync(index, 'utf8')) : [];
const seen = new Set(list.map((c) => c.url));
for (const q of QUERIES) {
  const url = `https://api.openverse.org/v1/images/?q=${encodeURIComponent(q)}&license=cc0,pdm&page_size=20&mature=false`;
  const r = await fetch(url, { headers: { 'User-Agent': UA } });
  if (!r.ok) {
    console.warn(`Openverse ${r.status} for ${q}`);
    await sleep(5000);
    continue;
  }
  let got = 0;
  for (const item of (await r.json()).results ?? []) {
    if (got >= PER || seen.has(item.url)) continue;
    seen.add(item.url);
    const res = await fetch(item.url, { headers: { 'User-Agent': UA } }).catch(() => null);
    if (!res?.ok || !(res.headers.get('content-type') ?? '').startsWith('image/jpeg')) continue;
    const buf = Buffer.from(await res.arrayBuffer());
    if (buf.length < 20_000 || buf.length > 15_000_000) continue;
    const file = `c${String(list.length + 1).padStart(3, '0')}.jpg`;
    writeFileSync(join(out, file), buf);
    list.push({ file, url: item.url, query: q, license: item.license, license_url: item.license_url, creator: item.creator ?? null, source: item.foreign_landing_url ?? null });
    got++;
  }
  console.log(`${q}: ${got}`);
}
writeFileSync(index, JSON.stringify(list, null, 1));
console.log(`${list.length} candidates in ${out}`);
