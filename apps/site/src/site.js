// Theme switch, the menu on small screens, the guide's outline, and the
// download links of the newest release (from GitHub's API; the page links
// to the releases page when that is not reachable).

const REPO = 'mrghxst/roll';
const store = {
  get: (k) => {
    try {
      return localStorage.getItem(k);
    } catch {
      return null;
    }
  },
  set: (k, v) => {
    try {
      localStorage.setItem(k, v);
    } catch {
      /* private window */
    }
  },
};

// Light or dark: follows the system until chosen.
for (const b of document.querySelectorAll('[data-theme-toggle]')) {
  b.addEventListener('click', () => {
    const root = document.documentElement;
    const dark = root.dataset.theme ? root.dataset.theme === 'dark' : matchMedia('(prefers-color-scheme: dark)').matches;
    root.dataset.theme = dark ? 'light' : 'dark';
    store.set('roll-theme', root.dataset.theme);
  });
}

// Menu.
const menu = document.querySelector('[data-menu]');
const links = document.getElementById('links');
menu?.addEventListener('click', () => {
  const open = links.classList.toggle('open');
  menu.setAttribute('aria-expanded', String(open));
});
links?.addEventListener('click', (e) => {
  if (e.target.closest('a')) {
    links.classList.remove('open');
    menu?.setAttribute('aria-expanded', 'false');
  }
});

// A line under the top bar once the page scrolls.
const bar = document.querySelector('.top');
const onScroll = () => bar?.classList.toggle('scrolled', scrollY > 8);
addEventListener('scroll', onScroll, { passive: true });
onScroll();

// The guide's outline marks the section in view.
const toc = [...document.querySelectorAll('.docs-toc a')];
if (toc.length && 'IntersectionObserver' in window) {
  const byId = new Map(toc.map((a) => [a.getAttribute('href').slice(1), a]));
  const seen = new Set();
  const io = new IntersectionObserver(
    (entries) => {
      for (const e of entries) e.isIntersecting ? seen.add(e.target.id) : seen.delete(e.target.id);
      const first = [...byId.keys()].find((id) => seen.has(id));
      if (!first) return;
      for (const a of toc) a.classList.toggle('on', a === byId.get(first));
    },
    { rootMargin: '-80px 0px -60% 0px' },
  );
  for (const id of byId.keys()) {
    const h = document.getElementById(id);
    if (h) io.observe(h);
  }
}

// Downloads: which system this is, and the newest release's files.
const os = (() => {
  const p = (navigator.userAgentData?.platform || navigator.platform || navigator.userAgent).toLowerCase();
  if (p.includes('win')) return 'windows';
  if (p.includes('mac')) return 'mac';
  if (p.includes('linux') && !/android/.test(navigator.userAgent.toLowerCase())) return 'linux';
  return null;
})();
const OS_NAME = { windows: 'Windows', mac: 'macOS', linux: 'Linux' };
const mb = (b) => `${Math.round(b / 1048576)} MB`;

const cards = [...document.querySelectorAll('.dl[data-asset]')];
const mine = cards.find((c) => c.dataset.os === os);
mine?.classList.add('mine');
if (os) for (const b of document.querySelectorAll('[data-primary-download]')) b.textContent = `Download for ${OS_NAME[os]}`;

if (cards.length) {
  fetch(`https://api.github.com/repos/${REPO}/releases?per_page=5`, { headers: { Accept: 'application/vnd.github+json' } })
    .then((r) => (r.ok ? r.json() : Promise.reject(new Error(String(r.status)))))
    .then((releases) => {
      const rel = releases.find((r) => !r.draft && r.assets?.length);
      if (!rel) return;
      for (const c of cards) {
        const asset = rel.assets.find((a) => a.name.endsWith(c.dataset.asset));
        if (!asset) continue;
        c.href = asset.browser_download_url;
        const size = c.querySelector('[data-size]');
        if (size) size.textContent = `${size.textContent} · ${mb(asset.size)}`;
      }
      const ver = rel.tag_name.replace(/^v/, '');
      for (const v of document.querySelectorAll('[data-version]')) v.textContent = `Version ${ver}${rel.prerelease ? ', pre-release' : ''}`;
      for (const a of document.querySelectorAll('[data-release-link]')) a.href = rel.html_url;
      if (mine) {
        for (const b of document.querySelectorAll('[data-primary-download]')) {
          b.href = mine.href;
          b.textContent = `Download for ${OS_NAME[os]} · ${ver}`;
        }
      }
    })
    .catch(() => {
      /* The cards keep linking to the releases page. */
    });
}
