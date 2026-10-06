// The formatter of the interface's code and of the repository's scripts: `npm run format` rewrites them, and CI fails
// on any file `npm run format:check` would change. 120 columns, the width the comments of the code are wrapped at;
// single quotes, the ones most of the code already used.
export default {
  plugins: ['prettier-plugin-svelte'],
  printWidth: 120,
  singleQuote: true,
};
