import { defineConfig } from 'vitest/config';

// Unit tests cover the framework-free logic modules (`$lib/format`,
// `$lib/stacktrace`). They need no DOM or the SvelteKit plugin, so this is a
// standalone config running in a plain Node environment.
export default defineConfig({
  test: {
    include: ['src/**/*.test.ts'],
    environment: 'node'
  }
});
