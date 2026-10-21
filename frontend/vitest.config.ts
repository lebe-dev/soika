import precompileIntl from 'svelte-intl-precompile/sveltekit-plugin';
import { svelte, vitePreprocess } from '@sveltejs/vite-plugin-svelte';
import { fileURLToPath } from 'node:url';
import { defineConfig } from 'vitest/config';

// Vitest config for the frontend unit tests.
//
// We deliberately use the lightweight `svelte()` plugin (not the full
// `sveltekit()` plugin) so that:
//   - Svelte 5 rune modules (`*.svelte.ts`, e.g. `$lib/stores/auth.svelte.ts`)
//     are compiled and `$state`/`$derived` work in tests.
//   - Pure helper modules (`$lib/format`, `$lib/stacktrace`, the api client)
//     keep running in a fast plain-Node environment.
//   - Route `load` functions that `import { redirect, error } from
//     '@sveltejs/kit'` resolve — those are plain JS exports that run in Node.
//
// What this config does NOT provide is the SvelteKit virtual modules
// (`$app/*`, `./$types`). `./$types` is type-only and erased at compile time.
// Component tests mock `$app/stores`; a test-only alias makes that virtual
// module resolvable without loading the SvelteKit runtime.
//
// Environment: the default is `node` (fast, no DOM). Tests that genuinely need
// a DOM must opt in per-file with a docblock at the very top of the file:
//
//     // @vitest-environment jsdom
//
// `$lib` is aliased to `src/lib` to mirror the SvelteKit alias in
// `svelte.config.js`.
export default defineConfig({
  plugins: [precompileIntl('locales'), svelte({ preprocess: vitePreprocess() })],
  resolve: {
    alias: {
      $lib: fileURLToPath(new URL('./src/lib', import.meta.url))
    },
    // Let Svelte/runes resolve their browser entry points when a test opts
    // into the jsdom environment; harmless for the Node helper tests.
    conditions: ['browser']
  },
  test: {
    alias: {
      '$app/stores': fileURLToPath(new URL('./src/test/app-module.ts', import.meta.url)),
      '$app/navigation': fileURLToPath(new URL('./src/test/app-navigation.ts', import.meta.url))
    },
    include: ['src/**/*.test.ts'],
    environment: 'node',
    coverage: {
      provider: 'v8',
      // `lcov` emits `coverage/lcov.info`, which SonarQube reads
      // (`sonar.javascript.lcov.reportPaths`); `text` keeps the console table.
      reporter: ['text', 'lcov'],
      include: ['src/**/*.{ts,svelte.ts}'],
      // `components/ui` is vendored shadcn-svelte — not our code to test.
      exclude: ['src/**/*.test.ts', 'src/**/*.d.ts', 'src/lib/components/ui/**']
    }
  }
});
