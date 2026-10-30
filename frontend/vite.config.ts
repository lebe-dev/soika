import precompileIntl from 'svelte-intl-precompile/sveltekit-plugin';
import { sveltekit } from '@sveltejs/kit/vite';
import tailwindcss from '@tailwindcss/vite';
import { defineConfig } from 'vite';
import { execFileSync } from 'node:child_process';
import kitConfig from './sveltekit.config.js';

let gitCommit = process.env.GIT_COMMIT ?? '';
if (!gitCommit) {
  try {
    gitCommit = execFileSync('git', ['rev-parse', '--short', 'HEAD'], {
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'ignore']
    }).trim();
  } catch {
    // Source archives may not contain Git metadata.
  }
}

export default defineConfig({
  define: { __GIT_COMMIT__: JSON.stringify(gitCommit) },
  plugins: [precompileIntl('locales'), tailwindcss(), sveltekit(kitConfig)],
  server: {
    allowedHosts: ['test.home'],
    // Dev convenience: proxy API calls to the Rust backend so the session
    // cookie (same-origin) works during `yarn dev`. The built SPA is served
    // from the same origin as the API in production, so no proxy is needed.
    //
    // Only true backend namespaces are proxied — the whole JSON API lives under
    // `/api/*`, auth at `/auth/*`. Everything else (`/teams/{id}`, `/projects`,
    // `/invite/{token}`, …) is an SPA client route served by Vite/SvelteKit.
    proxy: {
      '/api': 'http://localhost:18080',
      '/auth': 'http://localhost:18080',
      '/healthz': 'http://localhost:18080'
    }
  }
});
