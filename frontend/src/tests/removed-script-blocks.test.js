import { describe, it, expect } from 'vitest';
import { findRemovedScriptBlock } from '../lib/removed-script-blocks.js';

const config = (rule) => ({
  services: [
    {
      name: 'svc',
      rules: [
        { name: 'ok', script: '1' },
        { name: 'old', ...rule },
      ],
    },
  ],
});

describe('findRemovedScriptBlock', () => {
  it('lets a configuration through when the removed blocks are absent, null or blank', () => {
    expect(findRemovedScriptBlock(config({}))).toBeNull();
    expect(findRemovedScriptBlock(config({ pre_script: null, post_script: null }))).toBeNull();
    expect(findRemovedScriptBlock(config({ pre_script: '', post_script: '  \n' }))).toBeNull();
    expect(findRemovedScriptBlock({ services: [] })).toBeNull();
  });

  it('names the service, the rule and the block that still hold code', () => {
    expect(findRemovedScriptBlock(config({ pre_script: '40 + 2' }))).toEqual({
      service: 'svc',
      rule: 'old',
      block: 'pre_script',
    });
    expect(findRemovedScriptBlock(config({ post_script: '#{ a: 1 }' }))).toMatchObject({ block: 'post_script' });
  });

  it('refuses a block that is not text rather than guess what it held', () => {
    expect(findRemovedScriptBlock(config({ post_script: 42 }))).toMatchObject({ block: 'post_script' });
  });
});
