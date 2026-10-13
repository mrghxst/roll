import { createReadStream, existsSync } from 'node:fs';
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

const host = process.env.TAURI_DEV_HOST;

export default defineConfig({
  plugins: [svelte(), cc0Pictures()],
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
