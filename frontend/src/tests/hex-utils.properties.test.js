// Properties of hex-utils.js on generated inputs (fast-check). The server's fuzz target tcp_hex checks the same round
// trip on src/tcp/hex.rs: what the UI encodes, the server reads back as the same bytes.
import { describe, it, expect } from 'vitest';
import fc from 'fast-check';
import { textToHex, hexToBytes, hexToTextOrNull, isValidHex } from '../lib/hex-utils.js';

describe('hex-utils properties', () => {
  it('any text encodes to valid lowercase hexadecimal that decodes back to the same text', () => {
    fc.assert(
      fc.property(fc.string({ unit: 'grapheme' }), (text) => {
        const hex = textToHex(text);
        expect(isValidHex(hex)).toBe(true);
        expect(hex).toBe(hex.toLowerCase());
        expect(hexToTextOrNull(hex)).toBe(text);
      }),
    );
  });

  it('hexToBytes accepts exactly the valid hexadecimal, and its bytes encode back to it', () => {
    const nearlyHex = fc.string({ unit: fc.constantFrom(...'0123456789abcdefABCDEFgz '), maxLength: 40 });
    fc.assert(
      fc.property(fc.oneof(nearlyHex, fc.string()), (text) => {
        const bytes = hexToBytes(text);
        expect(bytes === null).toBe(!isValidHex(text));
        if (bytes !== null) {
          expect(Array.from(bytes, (b) => b.toString(16).padStart(2, '0')).join('')).toBe(text.toLowerCase());
        }
      }),
    );
  });
});
