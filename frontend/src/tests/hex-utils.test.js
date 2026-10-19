import { describe, it, expect } from 'vitest';
import { textToHex, hexToBytes, hexToTextOrNull, isValidHex } from '../lib/hex-utils.js';

describe('hex-utils', () => {
  it('textToHex encodes "pong" as the server does (`tcp::hex::encode`)', () => {
    expect(textToHex('pong')).toBe('706f6e67');
  });

  it('textToHex turns an empty string into an empty string', () => {
    expect(textToHex('')).toBe('');
  });

  it('textToHex encodes multi-byte UTF-8', () => {
    expect(textToHex('é')).toBe('c3a9');
  });

  it('hexToBytes decodes valid hexadecimal', () => {
    expect(Array.from(hexToBytes('706f6e67'))).toEqual([0x70, 0x6f, 0x6e, 0x67]);
  });

  it('hexToBytes accepts upper and lower case', () => {
    expect(Array.from(hexToBytes('AB'))).toEqual([0xab]);
    expect(Array.from(hexToBytes('ab'))).toEqual([0xab]);
  });

  it('hexToBytes returns null on an odd length', () => {
    expect(hexToBytes('abc')).toBeNull();
  });

  it('hexToBytes returns null on an invalid character', () => {
    expect(hexToBytes('zz')).toBeNull();
  });

  it('hexToBytes returns null when only the second digit of a pair is invalid, or the pair has a sign or a space', () => {
    // parseInt stops at the first character that is not a digit: '1z' would read as 1, '-1' as 255 once in a byte.
    for (const hex of ['1z', 'a ', ' a', '+1', '-1', '0x']) {
      expect(hexToBytes(hex), hex).toBeNull();
      expect(hexToTextOrNull(hex), hex).toBeNull();
    }
  });

  it('hexToTextOrNull reverses textToHex', () => {
    expect(hexToTextOrNull(textToHex('hello world'))).toBe('hello world');
  });

  it('hexToTextOrNull returns null on invalid hexadecimal', () => {
    expect(hexToTextOrNull('zz')).toBeNull();
  });

  it('hexToTextOrNull returns null on bytes that are not valid UTF-8', () => {
    // 0xff never starts a valid UTF-8 sequence.
    expect(hexToTextOrNull('ff')).toBeNull();
  });

  it('isValidHex accepts an empty string (no bytes)', () => {
    expect(isValidHex('')).toBe(true);
  });

  it('isValidHex rejects an odd length', () => {
    expect(isValidHex('abc')).toBe(false);
  });

  it('isValidHex rejects a character outside hexadecimal', () => {
    expect(isValidHex('zz')).toBe(false);
  });
});
