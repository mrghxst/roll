// Builds the CC0 test set (PROMPT 9.1): real photos under CC0 or public
// domain marks from Openverse, plus screenshots, memes, social media posts
// and junk shots rendered in a headless browser from those photos and our
// own text (released as CC0). Ground truth and licenses go to
// manifest.json. Output: fixtures-out/cc0/ (not in git).
//
//   cd apps/desktop && node ../../tools/cc0/build.mjs [--per 1.0]
//
// Openverse needs no key; requests carry a user agent naming the project.

import { mkdirSync, readFileSync, rmSync, writeFileSync, existsSync } from 'node:fs';
import { createRequire } from 'node:module';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
// Playwright comes from the desktop app's dependencies.
const { chromium } = createRequire(join(root, 'apps', 'desktop', 'package.json'))('@playwright/test');
const out = join(root, 'fixtures-out', 'cc0');
const scale = Number(process.argv[process.argv.indexOf('--per') + 1] || 1) || 1;
const UA = 'roll-fixtures/0.1 (https://github.com/mrghxst/roll; test set builder)';

// Openverse queries per category. Only CC0 and the public domain mark.
const QUERIES = {
  photos: ['dog', 'cat portrait', 'family picnic', 'city street', 'food plate', 'beach', 'mountain hike', 'birthday party', 'children playing', 'lake sunset'],
  documents: ['document scan', 'typed letter', 'receipt', 'printed form', 'handwritten letter', 'newspaper page'],
  wallpapers: ['abstract background', 'gradient texture', 'nebula', 'pattern seamless'],
};
const COUNTS = { photos: 40, documents: 25, wallpapers: 20, screenshots: 25, memes: 25, social_media: 20, junk: 15 };
const n = (k) => Math.max(2, Math.round(COUNTS[k] * scale));

let seed = 7;
const rnd = () => ((seed = (seed * 1103515245 + 12345) % 2147483648) / 2147483648);
const pick = (a) => a[Math.floor(rnd() * a.length)];
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function search(q, page) {
  const url = `https://api.openverse.org/v1/images/?q=${encodeURIComponent(q)}&license=cc0,pdm&page_size=20&page=${page}&mature=false`;
  const r = await fetch(url, { headers: { 'User-Agent': UA } });
  if (!r.ok) throw new Error(`Openverse ${r.status} for ${q}`);
  return (await r.json()).results ?? [];
}

async function download(item) {
  const r = await fetch(item.url, { headers: { 'User-Agent': UA } });
  if (!r.ok) return null;
  const type = r.headers.get('content-type') ?? '';
  if (!type.startsWith('image/jpeg') && !type.startsWith('image/png')) return null;
  const buf = Buffer.from(await r.arrayBuffer());
  return buf.length > 20_000 && buf.length < 15_000_000 ? buf : null;
}

// Images checked by eye: a better category, or "drop" for ones that fit
// none clearly. Keyed by source URL.
const review = JSON.parse(readFileSync(join(root, 'tools', 'cc0', 'review.json'), 'utf8'));
const manifest = [];
const counter = {};
const nextFile = (cat) => `${cat}/${cat}-${String((counter[cat] = (counter[cat] ?? 0) + 1)).padStart(3, '0')}.jpg`;
const seen = new Set();

async function fetchCategory(cat, browser) {
  const want = n(cat);
  const queries = QUERIES[cat];
  let got = 0;
  for (let round = 1; got < want && round <= 3; round++) {
    for (const q of queries) {
      if (got >= want) break;
      let results = [];
      try {
        results = await search(q, round);
      } catch (e) {
        console.warn(String(e));
        await sleep(5000);
        continue;
      }
      // A few per query keeps the set varied.
      for (const item of results.filter((i) => !seen.has(i.id)).slice(0, Math.ceil(want / queries.length) + 1)) {
        if (got >= want) break;
        seen.add(item.id);
        const verdict = review[item.url]?.category ?? cat;
        // A dropped image still counts, so the set stays the reviewed one.
        if (verdict === 'drop') {
          got++;
          continue;
        }
        const buf = await download(item);
        if (!buf) continue;
        const file = nextFile(verdict);
        await toJpeg(browser, buf, join(out, file));
        manifest.push({
          file,
          category: verdict,
          searched: cat,
          source: { kind: 'openverse', provider: item.provider, url: item.url, landing: item.foreign_landing_url, creator: item.creator ?? null, license: item.license, license_version: item.license_version ?? null, title: item.title ?? null },
        });
        got++;
        await sleep(300);
      }
      await sleep(1200);
    }
  }
  console.log(`${cat}: ${got}`);
}

/** Re-encode through the browser: at most 2048 px, JPEG, no metadata. */
async function toJpeg(browser, buf, path) {
  const page = await browser.newPage();
  const b64 = buf.toString('base64');
  const data = await page.evaluate(async (src) => {
    const img = new Image();
    img.src = src;
    await img.decode();
    const s = Math.min(1, 2048 / Math.max(img.naturalWidth, img.naturalHeight));
    const c = document.createElement('canvas');
    c.width = Math.round(img.naturalWidth * s);
    c.height = Math.round(img.naturalHeight * s);
    c.getContext('2d').drawImage(img, 0, 0, c.width, c.height);
    return c.toDataURL('image/jpeg', 0.9);
  }, `data:image/jpeg;base64,${b64}`);
  await page.close();
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, Buffer.from(data.split(',')[1], 'base64'));
}

async function render(browser, html, w, h, path, type = 'png') {
  const page = await browser.newPage({ viewport: { width: w, height: h }, deviceScaleFactor: 2 });
  await page.setContent(`<!doctype html><meta charset="utf-8"><style>*{box-sizing:border-box}body{margin:0;font-family:"Segoe UI",system-ui,sans-serif}</style>${html}`);
  await page.waitForTimeout(50);
  mkdirSync(dirname(path), { recursive: true });
  await page.screenshot({ path, type, ...(type === 'jpeg' ? { quality: 85 } : {}) });
  await page.close();
}

// Only photos found as photos: relabelled ones may be anything.
const photoFiles = () => manifest.filter((m) => m.category === 'photos' && m.searched === 'photos').map((m) => m.file);
const dataUrl = async (file) => `data:image/jpeg;base64,${(await import('node:fs')).readFileSync(join(out, file)).toString('base64')}`;

const CHATS = [
  ['Mum', 'Are you coming on Sunday?', 'Yes, around 12. Should I bring dessert?', 'That would be lovely, the kids want cake'],
  ['Alex', 'Did you send the tickets?', 'Just now, check your email', 'Got them, thanks!'],
  ['Team', 'Standup moved to 10:30', 'ok', 'Can someone share the slides from yesterday?'],
  ['Sam', 'Look at this view', 'Wow where is that?', 'Lake hike, about 2 hours from town'],
];
const SETTINGS = ['Wi-Fi', 'Bluetooth', 'Mobile data', 'Notifications', 'Sounds & haptics', 'Focus', 'Screen time', 'General', 'Display & brightness', 'Battery', 'Privacy & security'];
const MAILS = ['Your order has shipped', 'Invoice for September', 'Meeting notes', 'Photos from the weekend', 'Password changed', 'Flight check-in is open', 'Weekly report'];

function screenshotHtml(i, dark) {
  const bg = dark ? '#111' : '#fff';
  const fg = dark ? '#eee' : '#111';
  const mut = dark ? '#999' : '#666';
  const bar = `<div style="display:flex;justify-content:space-between;padding:10px 18px;font-weight:600;font-size:15px">${9 + (i % 12)}:${String((i * 7) % 60).padStart(2, '0')}<span>5G ▮▮▮ ${40 + (i % 60)}%</span></div>`;
  const kind = i % 3;
  let body = '';
  if (kind === 0) {
    const c = CHATS[i % CHATS.length];
    body = `<div style="padding:8px 16px;font-weight:600;font-size:18px;border-bottom:1px solid ${mut}">${c[0]}</div>` +
      c.slice(1).map((m, k) => `<div style="margin:10px 14px;display:flex;justify-content:${k % 2 ? 'flex-end' : 'flex-start'}"><span style="max-width:70%;padding:10px 14px;border-radius:18px;background:${k % 2 ? '#2b7cff' : dark ? '#2a2a2a' : '#eee'};color:${k % 2 ? '#fff' : fg};font-size:16px">${m}</span></div>`).join('');
  } else if (kind === 1) {
    body = `<div style="padding:8px 16px;font-size:30px;font-weight:700">Settings</div>` +
      SETTINGS.map((s) => `<div style="padding:13px 16px;border-bottom:1px solid ${dark ? '#222' : '#eee'};font-size:16px;display:flex;justify-content:space-between">${s}<span style="color:${mut}">›</span></div>`).join('');
  } else {
    body = `<div style="padding:8px 16px;font-size:28px;font-weight:700">Inbox</div>` +
      MAILS.map((m, k) => `<div style="padding:10px 16px;border-bottom:1px solid ${dark ? '#222' : '#eee'}"><div style="font-weight:600;font-size:15px">${['Shop', 'Accounts', 'Jordan', 'Kim', 'Security', 'Airline', 'Office'][k]}</div><div style="font-size:15px">${m}</div><div style="color:${mut};font-size:13px">Preview text for ${m.toLowerCase()}…</div></div>`).join('');
  }
  return `<div style="width:390px;height:844px;background:${bg};color:${fg};overflow:hidden">${bar}${body}</div>`;
}

const CAPTIONS = [
  ['When the code works', 'on the first try'],
  ['Me waiting for the weekend', ''],
  ['Nobody:', 'My cat at 3 am'],
  ['One does not simply', 'leave the house without snacks'],
  ['Monday morning', 'expectation vs reality'],
  ['When someone says', 'it is only a five minute walk'],
];

async function main() {
  if (existsSync(out)) rmSync(out, { recursive: true, force: true });
  mkdirSync(out, { recursive: true });
  const browser = await chromium.launch();
  for (const cat of Object.keys(QUERIES)) await fetchCategory(cat, browser);

  const photos = photoFiles();
  // Screenshots: phone screens with our own text.
  for (let i = 0; i < n('screenshots'); i++) {
    const file = `screenshots/screenshots-${String(i + 1).padStart(3, '0')}.png`;
    await render(browser, screenshotHtml(i, i % 4 === 3), 390, 844, join(out, file));
    manifest.push({ file, category: 'screenshots', source: { kind: 'rendered', license: 'cc0', note: 'Rendered by tools/cc0/build.mjs' } });
  }
  // Memes: CC0 photos with captions.
  for (let i = 0; i < n('memes'); i++) {
    const [top, bottom] = CAPTIONS[i % CAPTIONS.length];
    const from = photos[i % photos.length];
    const src = await dataUrl(from);
    const style = 'position:absolute;left:0;right:0;text-align:center;font-family:Impact,Arial Black,sans-serif;font-size:44px;color:#fff;text-transform:uppercase;-webkit-text-stroke:2px #000;padding:0 12px';
    const html = i % 3 === 2
      ? `<div style="width:600px;background:#fff"><div style="padding:16px 20px;font-size:28px;font-family:Arial,sans-serif">${top} ${bottom}</div><img src="${src}" style="width:600px;display:block"></div>`
      : `<div style="position:relative;width:600px;height:600px;overflow:hidden"><img src="${src}" style="width:600px;height:600px;object-fit:cover"><div style="${style};top:10px">${top}</div><div style="${style};bottom:10px">${bottom}</div></div>`;
    const file = `memes/memes-${String(i + 1).padStart(3, '0')}.jpg`;
    await render(browser, html, 600, i % 3 === 2 ? 680 : 600, join(out, file), 'jpeg');
    manifest.push({ file, category: 'memes', source: { kind: 'rendered', license: 'cc0', derived_from: from } });
  }
  // Social media posts: the photo inside an app's post frame.
  for (let i = 0; i < n('social_media'); i++) {
    const from = photos[(i * 3 + 1) % photos.length];
    const src = await dataUrl(from);
    const user = pick(['travel.diaries', 'foodie_lena', 'daily.dose', 'outdoor.co', 'cityscapes_']);
    const html = `<div style="width:420px;background:#fff;color:#111;font-size:14px"><div style="display:flex;align-items:center;gap:8px;padding:10px"><span style="width:32px;height:32px;border-radius:50%;background:#c13584"></span><b>${user}</b><span style="margin-left:auto">•••</span></div><img src="${src}" style="width:420px;height:420px;object-fit:cover;display:block"><div style="padding:8px 10px">♡ 💬 ➤<div><b>${1000 + i * 37} likes</b></div><div><b>${user}</b> Weekend vibes #${pick(['travel', 'food', 'nature', 'city'])} #photooftheday</div><div style="color:#888">View all ${10 + i} comments</div></div></div>`;
    const file = `social_media/social_media-${String(i + 1).padStart(3, '0')}.jpg`;
    await render(browser, html, 420, 560, join(out, file), 'jpeg');
    manifest.push({ file, category: 'social_media', source: { kind: 'rendered', license: 'cc0', derived_from: from } });
  }
  // Junk: pocket shots, motion blur, a covered lens.
  for (let i = 0; i < n('junk'); i++) {
    const from = photos[(i * 5 + 2) % photos.length];
    const src = await dataUrl(from);
    const filter = ['blur(18px) brightness(0.25)', 'blur(10px)', 'brightness(0.06)', 'blur(25px) saturate(0.3)'][i % 4];
    const html = `<div style="width:600px;height:800px;overflow:hidden;background:#000"><img src="${src}" style="width:600px;height:800px;object-fit:cover;filter:${filter};transform:rotate(${(i % 5) * 9}deg) scale(1.3)"></div>`;
    const file = `junk/junk-${String(i + 1).padStart(3, '0')}.jpg`;
    await render(browser, html, 600, 800, join(out, file), 'jpeg');
    manifest.push({ file, category: 'junk', source: { kind: 'rendered', license: 'cc0', derived_from: from } });
  }
  await browser.close();

  writeFileSync(join(out, 'manifest.json'), JSON.stringify({ created: new Date().toISOString(), items: manifest }, null, 1));
  writeFileSync(
    join(out, 'LICENSES.md'),
    `# CC0 test set\n\nPhotos, documents and wallpapers are from Openverse under CC0 or the Public Domain Mark; each one's source, creator and license is in manifest.json. Screenshots, memes, social media posts and junk shots were rendered by tools/cc0/build.mjs and are released under CC0.\n`,
  );
  const counts = manifest.reduce((m, x) => ((m[x.category] = (m[x.category] ?? 0) + 1), m), {});
  console.log(JSON.stringify(counts));
}

await main();
