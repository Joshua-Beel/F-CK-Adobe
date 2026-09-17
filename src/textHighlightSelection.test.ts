import { describe, expect, it } from 'vitest';
import { wholeGlyphRange } from './textHighlightSelection';

describe('wholeGlyphRange', () => {
  it('expands UTF-16 endpoints to whole geometry characters, including an astral glyph', () => {
    expect(wholeGlyphRange({ index: 4, offset: 1, length: 2 }, { index: 4, offset: 1, length: 2 })).toEqual({ start: 4, end: 5 });
  });

  it('keeps original geometry-array whitespace indexes and normalizes reversed endpoints', () => {
    expect(wholeGlyphRange({ index: 7, offset: 1, length: 1 }, { index: 3, offset: 0, length: 1 })).toEqual({ start: 3, end: 8 });
  });

  it('rejects collapsed, invalid, and out-of-glyph endpoints', () => {
    expect(wholeGlyphRange({ index: 3, offset: 0, length: 1 }, { index: 3, offset: 0, length: 1 })).toBeNull();
    expect(wholeGlyphRange({ index: 3, offset: 2, length: 1 }, { index: 4, offset: 0, length: 1 })).toBeNull();
  });
});
