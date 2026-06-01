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
    proxy: {
      '/api': 'http://localhost:18080',
      '/auth': 'http://localhost:18080',
      '/invite': 'http://localhost:18080',
      '/projects': 'http://localhost:18080',
      '/issues': 'http://localhost:18080',
      '/events': 'http://localhost:18080',
      '/teams': 'http://localhost:18080',
      '/profile': 'http://localhost:18080',
      '/settings': 'http://localhost:18080',
      '/admin': 'http://localhost:18080',
      '/healthz': 'http://localhost:18080'
    }
  }
});
