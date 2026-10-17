// Builds the website (GitHub Pages) into apps/site/dist: the landing page
// from src/index.html and the guide from docs/user-guide.md and
// docs/immich.md, so the docs have one source. Every link is relative, so
// the site works under /roll/ and from a plain folder.
//
//   node apps/site/build.mjs            # build
//   node apps/site/build.mjs --serve    # build and serve on http://localhost:4321

import { cpSync, existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:http';
import { createRequire } from 'node:module';
import { dirname, extname, join, normalize, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { Marked } from 'marked';

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(here, '../..');
const dist = join(here, 'dist');
const REPO = 'https://github.com/mrghxst/roll';
const require = createRequire(import.meta.url);

/** The guide's pages: source, output folder, title in the sidebar. */
const DOCS = [
  { src: 'docs/user-guide.md', out: 'guide', nav: 'User guide' },
  { src: 'docs/immich.md', out: 'guide/immich', nav: 'Roll and Immich' },
];

const esc = (s) => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
const slug = (s) =>
  s
    .toLowerCase()
    .replace(/<[^>]+>/g, '')
    .replace(/&[a-z]+;/g, '')
    .replace(/[^\p{L}\p{N}]+/gu, '-')
    .replace(/^-|-$/g, '');

const ICON = {
  github:
    '<svg viewBox="0 0 24 24" width="18" height="18" aria-hidden="true"><path fill="currentColor" d="M12 .5a11.5 11.5 0 0 0-3.64 22.41c.58.1.79-.25.79-.56v-2c-3.2.7-3.88-1.37-3.88-1.37-.52-1.33-1.28-1.69-1.28-1.69-1.05-.71.08-.7.08-.7 1.16.08 1.77 1.19 1.77 1.19 1.03 1.77 2.7 1.26 3.36.96.1-.75.4-1.26.73-1.55-2.55-.29-5.24-1.28-5.24-5.69 0-1.26.45-2.29 1.19-3.09-.12-.29-.52-1.46.11-3.05 0 0 .97-.31 3.17 1.18a11 11 0 0 1 5.77 0c2.2-1.49 3.17-1.18 3.17-1.18.63 1.59.23 2.76.11 3.05.74.8 1.19 1.83 1.19 3.09 0 4.42-2.7 5.39-5.26 5.68.41.36.78 1.06.78 2.14v3.17c0 .31.21.67.8.56A11.5 11.5 0 0 0 12 .5Z"/></svg>',
  theme:
    '<svg viewBox="0 0 24 24" width="18" height="18" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.6"><circle cx="12" cy="12" r="8.5"/><path d="M12 3.5v17a8.5 8.5 0 0 0 0-17Z" fill="currentColor"/></svg>',
  menu: '<svg viewBox="0 0 24 24" width="20" height="20" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.6"><path d="M4 7h16M4 12h16M4 17h16"/></svg>',
};

/** The bar at the top of every page; `base` leads back to the site root. */
function header(base, active) {
  const link = (href, label, key) => `<a href="${base}${href}"${active === key ? ' aria-current="page"' : ''}>${label}</a>`;
  return `<header class="top">
  <div class="top-in">
    <a class="brand" href="${base}" aria-label="Roll, home"><img src="${base}logo.svg" width="26" height="26" alt=""><span>Roll</span></a>
    <nav class="links" id="links" aria-label="Site">
      ${link('#features', 'Features', 'features')}
      ${link('#map', 'Map', 'map')}
      ${link('#ai', 'AI', 'ai')}
      ${link('#privacy', 'Privacy', 'privacy')}
      ${link('guide/', 'Guide', 'guide')}
      ${link('#faq', 'FAQ', 'faq')}
    </nav>
    <div class="actions">
      <a class="icon-btn" href="${REPO}" aria-label="Roll on GitHub" title="Roll on GitHub">${ICON.github}</a>
      <button class="icon-btn" type="button" data-theme-toggle aria-label="Switch between light and dark" title="Light or dark">${ICON.theme}</button>
      <a class="btn btn-primary btn-sm" href="${base}#download">Download</a>
      <button class="icon-btn menu" type="button" aria-label="Menu" aria-expanded="false" aria-controls="links" data-menu>${ICON.menu}</button>
    </div>
  </div>
</header>`;
}

function footer(base) {
  return `<footer class="foot">
  <div class="foot-in">
    <div class="foot-brand">
      <img src="${base}logo.svg" width="22" height="22" alt="">
      <span>Roll</span>
      <p>A local photo and video library that sorts itself. MIT licensed.</p>
    </div>
    <nav aria-label="Footer">
      <div><h2>Roll</h2><a href="${base}#download">Download</a><a href="${REPO}/releases">All releases</a><a href="${base}#faq">FAQ</a></div>
      <div><h2>Docs</h2><a href="${base}guide/">User guide</a><a href="${base}guide/immich/">Roll and Immich</a><a href="${REPO}/blob/main/docs/plan.md">Roadmap</a></div>
      <div><h2>Project</h2><a href="${REPO}">Source code</a><a href="${REPO}/issues">Report a problem</a><a href="${REPO}/blob/main/docs/benchmarks.md">Benchmarks</a></div>
    </nav>
  </div>
  <p class="fine">The screenshots show Roll's demo mode with public-domain pictures from Openverse (CC0 and Public Domain Mark); none come from a real library. Map data © OpenStreetMap contributors, Protomaps; Natural Earth.</p>
</footer>`;
}

function page({ base, title, description, body, active, path }) {
  return `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>${esc(title)}</title>
<meta name="description" content="${esc(description)}">
<meta property="og:title" content="${esc(title)}">
<meta property="og:description" content="${esc(description)}">
<meta property="og:type" content="website">
<meta property="og:image" content="https://mrghxst.github.io/roll/shots/timeline-light.webp">
<meta name="theme-color" content="#ffffff" media="(prefers-color-scheme: light)">
<meta name="theme-color" content="#000000" media="(prefers-color-scheme: dark)">
<link rel="icon" href="${base}logo.svg" type="image/svg+xml">
<link rel="canonical" href="https://mrghxst.github.io/roll/${path}">
<link rel="preload" href="${base}fonts/geist-latin-wght-normal.woff2" as="font" type="font/woff2" crossorigin>
<link rel="stylesheet" href="${base}style.css">
<script>try{var t=localStorage.getItem('roll-theme');if(t)document.documentElement.dataset.theme=t}catch(e){}</script>
<script src="${base}site.js" defer></script>
</head>
<body>
<a class="skip" href="#main">Skip to the content</a>
${header(base, active)}
${body}
${footer(base)}
</body>
</html>
`;
}

/** Renders one doc to HTML with heading ids, and its h2/h3 outline. */
function renderDoc(doc, base) {
  const md = readFileSync(join(root, doc.src), 'utf8');
  const outline = [];
  const marked = new Marked({ gfm: true });
  marked.use({
    renderer: {
      heading({ tokens, depth }) {
        const text = this.parser.parseInline(tokens);
        const id = slug(text);
        if (depth === 2 || depth === 3) outline.push({ depth, id, text });
        if (depth === 1) return `<h1>${text}</h1>\n`;
        return `<h${depth} id="${id}"><a class="anchor" href="#${id}" aria-hidden="true" tabindex="-1">#</a>${text}</h${depth}>\n`;
      },
      link({ href, title, tokens }) {
        const text = this.parser.parseInline(tokens);
        let to = href;
        const other = DOCS.find((d) => d.src.endsWith(`/${href.split('#')[0]}`));
        if (other) to = `${base}${other.out}/${href.includes('#') ? `#${href.split('#')[1]}` : ''}`;
        else if (/^[\w./-]+\.md(#.*)?$/.test(href)) to = `${REPO}/blob/main/docs/${href}`;
        else if (!/^(https?:|#|mailto:)/.test(href)) to = `${REPO}/blob/main/${href}`;
        return `<a href="${esc(to)}"${title ? ` title="${esc(title)}"` : ''}>${text}</a>`;
      },
    },
  });
  const html = marked.parse(md);
  const h1 = /^#\s+(.+)$/m.exec(md)?.[1] ?? doc.nav;
  const lead = md.split('\n\n')[1]?.replace(/\s+/g, ' ').replace(/[*`[\]]/g, '') ?? '';
  return { html, outline, h1, lead };
}

function build() {
  rmSync(dist, { recursive: true, force: true });
  mkdirSync(dist, { recursive: true });
  cpSync(join(here, 'public'), dist, { recursive: true });
  cpSync(join(here, 'src', 'style.css'), join(dist, 'style.css'));
  cpSync(join(here, 'src', 'site.js'), join(dist, 'site.js'));
  // Geist and Geist Mono (OFL), Latin and Latin Extended, served with the site.
  mkdirSync(join(dist, 'fonts'), { recursive: true });
  for (const [pkg, files] of [
    ['@fontsource-variable/geist', ['geist-latin-wght-normal.woff2', 'geist-latin-ext-wght-normal.woff2']],
    ['@fontsource-variable/geist-mono', ['geist-mono-latin-wght-normal.woff2', 'geist-mono-latin-ext-wght-normal.woff2']],
  ]) {
    const dir = join(dirname(require.resolve(`${pkg}/package.json`)), 'files');
    for (const f of files) cpSync(join(dir, f), join(dist, 'fonts', f));
  }

  const body = readFileSync(join(here, 'src', 'index.html'), 'utf8');
  writeFileSync(
    join(dist, 'index.html'),
    page({
      base: '',
      path: '',
      active: null,
      title: 'Roll: your camera roll, sorted',
      description:
        'A free, open-source photo and video library for Windows, macOS and Linux that sorts a phone dump of 100,000 files into a clean archive by year, fixes dates and places in bulk, and never loses anything.',
      body,
    }),
  );

  for (const doc of DOCS) {
    const depth = doc.out.split('/').length;
    const base = '../'.repeat(depth);
    const { html, outline, h1, lead } = renderDoc(doc, base);
    const pages = DOCS.map(
      (d) => `<a href="${base}${d.out}/"${d === doc ? ' aria-current="page"' : ''}>${esc(d.nav)}</a>`,
    ).join('');
    const toc = outline
      .map((o) => `<a class="toc-${o.depth}" href="#${o.id}">${o.text.replace(/<[^>]+>/g, '')}</a>`)
      .join('');
    const main = `<main id="main" class="docs">
  <aside class="docs-side" aria-label="Guide">
    <nav class="docs-pages" aria-label="Guide pages">${pages}</nav>
    <nav class="docs-toc" aria-label="On this page"><h2>On this page</h2>${toc}</nav>
  </aside>
  <article class="prose">
${html}
    <p class="edit"><a href="${REPO}/edit/main/${doc.src}">Improve this page on GitHub</a></p>
  </article>
</main>`;
    mkdirSync(join(dist, doc.out), { recursive: true });
    writeFileSync(
      join(dist, doc.out, 'index.html'),
      page({ base, path: `${doc.out}/`, active: 'guide', title: `${h1.replace(/^Roll /, '')} · Roll`, description: lead.slice(0, 200), body: main }),
    );
  }
  writeFileSync(join(dist, '.nojekyll'), '');
  console.log(`site built in ${dist}`);
}

build();

if (process.argv.includes('--serve')) {
  const TYPES = { '.html': 'text/html; charset=utf-8', '.css': 'text/css', '.js': 'text/javascript', '.svg': 'image/svg+xml', '.webp': 'image/webp', '.woff2': 'font/woff2', '.png': 'image/png' };
  createServer((req, res) => {
    let path = decodeURIComponent((req.url ?? '/').split('?')[0]);
    path = normalize(join(dist, path.replace(/^\/roll\//, '/')));
    if (!path.startsWith(dist)) return void res.writeHead(403).end();
    if (existsSync(path) && !extname(path)) path = join(path, 'index.html');
    if (!existsSync(path)) return void res.writeHead(404).end('Not found');
    res.writeHead(200, { 'Content-Type': TYPES[extname(path)] ?? 'application/octet-stream' });
    res.end(readFileSync(path));
  }).listen(4321, () => console.log('http://localhost:4321/'));
}
