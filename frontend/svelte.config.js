import adapter from '@sveltejs/adapter-static';
import { vitePreprocess } from '@sveltejs/vite-plugin-svelte';

/** @type {import('@sveltejs/kit').Config} */
const config = {
  preprocess: vitePreprocess(),

  kit: {
    // The Rust binary embeds the built assets via rust-embed (#[folder = "web/dist"]).
    // adapter-static writes the SPA build (with a fallback document) into ../web/dist
    // so `cargo build` picks it up.
    adapter: adapter({
      pages: '../web/dist',
      assets: '../web/dist',
      fallback: 'index.html',
      precompress: false,
      strict: false
    }),
    alias: {
      $lib: 'src/lib'
    }
  }
};

export default config;
