import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: { ignored: ['**/src-tauri/**', '**/target/**'] },
  },
  preview: { port: 4173, strictPort: true },
  test: {
    environment: 'jsdom',
    setupFiles: ['./src/test/setup.ts'],
    css: true,
    // The delivery's component sheet is stubbed HERE ONLY, never in a build.
    //
    // Measured: one interaction-heavy test went 527ms -> 5,456ms when @redrob-labs/ui/styles.css joined
    // the app's own sheet, which tipped it past the 5s default and looked like four broken tests. The
    // cause is not the product -- `css: true` injects the sheet into jsdom, and jsdom re-matches all
    // 4,867 rules on every getComputedStyle that userEvent makes before each click.
    //
    // `css: true` stays, because the app's own rules are worth having in a test. Only the delivery's
    // component rules go, and the stub file says what that costs: a test cannot assert anything those
    // rules decide.
    alias: [{ find: '@redrob-labs/ui/styles.css', replacement: './src/test/design-system-styles.stub.css' }],
    coverage: {
      provider: 'v8',
      reporter: ['text', 'html', 'json-summary'],
      reportsDirectory: 'coverage',
      include: ['src/**/*.{ts,tsx}'],
      exclude: ['src/**/*.test.{ts,tsx}', 'src/test/**', 'src/main.tsx'],
      thresholds: { statements: 85, branches: 75, functions: 65, lines: 85 },
    },
  },
});
