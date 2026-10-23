import { svelte } from '@sveltejs/vite-plugin-svelte';
import { defineConfig } from 'vitest/config';

export default defineConfig({
  base: '/design/',
  plugins: [svelte()],
  build: { outDir: 'dist' },
  server: { proxy: { '/api': { target: 'http://127.0.0.1:8080', ws: true } } },
  test: { environment: 'node', include: ['src/**/*.test.ts'] },
});
