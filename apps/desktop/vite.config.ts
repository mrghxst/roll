import { createReadStream, existsSync, readdirSync } from 'node:fs';
import { join, normalize } from 'node:path';
import { fileURLToPath } from 'node:url';
import { defineConfig, type Plugin } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';

/**
 * Dev server only: serves the CC0 test set (`fixtures-out/cc0`, built by
 * tools/cc0/build.mjs and never committed) at /__cc0/ so the mock backend
 * can show real pictures with `?pics=cc0`, for screenshots. Builds never
 * see it.
 */
function cc0Pictures(): Plugin {
  const dir = normalize(fileURLToPath(new URL('../../fixtures-out/cc0/', import.meta.url)));
  const types: Record<string, string> = { json: 'application/json', jpg: 'image/jpeg', png: 'image/png' };
  return {
    name: 'roll-cc0-pictures',
    apply: 'serve',
    configureServer(server) {
      server.middlewares.use('/__cc0/', (req, res, next) => {
        const file = normalize(join(dir, decodeURIComponent((req.url ?? '').split('?')[0] ?? '')));
        const type = types[file.split('.').pop() ?? ''];
        if (!file.startsWith(dir) || !type || !existsSync(file)) return next();
        res.setHeader('Content-Type', type);
        createReadStream(file).pipe(res);
      });
    },
  };
}

/**
 * Dev server only: serves offline map areas from `fixtures-out/maps`
 * (`cargo run -p roll-core --example maparea`) at /__maps/ so the mock
 * backend can show them with `?maps=1`, the way Roll's `roll://maptile`
 * does. Uses Node's built-in SQLite.
 */
function offlineMaps(): Plugin {
  const dir = normalize(fileURLToPath(new URL('../../fixtures-out/maps/', import.meta.url)));
  return {
    name: 'roll-offline-maps',
    apply: 'serve',
    async configureServer(server) {
      if (!existsSync(dir)) return;
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      const { DatabaseSync } = (await import('node:sqlite' as string)) as any;
      const files = readdirSync(dir).filter((f) => f.endsWith('.db'));
      const dbs = files.map((f) => new DatabaseSync(join(dir, f), { readOnly: true }));
      server.middlewares.use('/__maps/', (req, res, next) => {
        const path = decodeURIComponent((req.url ?? '').split('?')[0] ?? '');
        if (path.replace(/^\//, '') === 'areas') {
          const areas = dbs.map((db, i) => {
            const meta = Object.fromEntries(db.prepare('SELECT key, value FROM meta').all().map((r: { key: string; value: string }) => [r.key, r.value]));
            return {
              id: files[i]!.replace(/\.db$/, ''),
              name: meta.name,
              bbox: JSON.parse(meta.bbox),
              max_zoom: Number(meta.max_zoom),
              tiles: Number(meta.tiles),
              bytes: Number(meta.bytes),
              state: 'ready',
              build: meta.build,
              error: null,
            };
          });
          res.setHeader('Content-Type', 'application/json');
          res.end(JSON.stringify(areas));
          return;
        }
        const tile = /^\/?tile\/(\d+)\/(\d+)\/(\d+)$/.exec(path);
        if (tile) {
          const [z, x, y] = tile.slice(1).map(Number);
          for (const db of dbs) {
            const row = db.prepare('SELECT data FROM tiles WHERE z = ? AND x = ? AND y = ?').get(z, x, y);
            if (row) {
              res.setHeader('Content-Type', 'application/x-protobuf');
              res.end(Buffer.from(row.data));
              return;
            }
          }
          res.statusCode = 204;
          res.end();
          return;
        }
        const font = normalize(join(dir, 'fonts', path.replace(/^\/?font\//, '')));
        if (path.includes('font/') && font.startsWith(dir) && existsSync(font)) {
          res.setHeader('Content-Type', 'application/x-protobuf');
          createReadStream(font).pipe(res);
          return;
        }
        next();
      });
    },
  };
}

const host = process.env.TAURI_DEV_HOST;

export default defineConfig({
  plugins: [svelte(), cc0Pictures(), offlineMaps()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: 'ws', host, port: 1421 } : undefined,
    watch: { ignored: ['**/src-tauri/**'] },
  },
  envPrefix: ['VITE_', 'TAURI_ENV_'],
  build: {
    target: 'es2023',
    sourcemap: false,
  },
  worker: { format: 'es' },
});
