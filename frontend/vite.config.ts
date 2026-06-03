import { sveltekit } from '@sveltejs/kit/vite';
import tailwindcss from '@tailwindcss/vite';
import { defineConfig } from 'vite';

export default defineConfig({
  plugins: [tailwindcss(), sveltekit()],
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
