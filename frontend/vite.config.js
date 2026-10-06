import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import { failOnWarning } from './svelte-warnings.js';

// A build fails on a warning of the Svelte compiler (svelte-warnings.js); the development server only logs them, so
// that work in progress keeps reloading.
export default defineConfig(({ command }) => ({
  plugins: [svelte(command === 'build' ? { onwarn: failOnWarning } : {})],
  server: {
    proxy: {
      '/api': 'http://localhost:7342',
      '/runtime-config.json': 'http://localhost:7342',
    },
  },
  build: {
    outDir: 'dist',
    emptyOutDir: true,
  },
}));
