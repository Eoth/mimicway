import { describe, it, expect, vi } from 'vitest';
import { compile } from 'svelte/compiler';
import { failOnWarning } from '../../svelte-warnings.js';

describe('warnings of the Svelte compiler', () => {
  it("fail the build in the interface's own files", () => {
    const { warnings } = compile('<img src="logo.png">', { filename: 'src/lib/components/Logo.svelte' });
    const missingAlt = warnings.find((w) => w.code === 'a11y_missing_attribute');
    expect(missingAlt).toBeDefined();
    const log = vi.fn();
    expect(() => failOnWarning(missingAlt, log)).toThrow(/Logo\.svelte:1: a11y_missing_attribute/);
    expect(log).not.toHaveBeenCalled();
  });

  it('only go to the log in dependencies', () => {
    const warning = { code: 'a11y_missing_attribute', message: 'no alt', filename: '/app/node_modules/lib/A.svelte' };
    const log = vi.fn();
    failOnWarning(warning, log);
    expect(log).toHaveBeenCalledWith(warning);
  });
});
