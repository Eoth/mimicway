// What a build does with a warning of the Svelte compiler. In the interface's own files it fails, so that a warning
// (accessibility, an unused selector, a misused rune...) is fixed, or silenced on purpose with a `svelte-ignore`
// comment where it occurs, instead of scrolling past in a log. Warnings in dependencies only go to the log.
export function failOnWarning(warning, log) {
  if (!warning.filename || /node_modules/.test(warning.filename)) {
    log(warning);
    return;
  }
  const where = warning.start ? `${warning.filename}:${warning.start.line}` : warning.filename;
  throw new Error(`${where}: ${warning.code}: ${warning.message}`);
}
