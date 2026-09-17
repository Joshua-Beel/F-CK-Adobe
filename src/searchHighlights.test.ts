import { describe, expect, it } from 'vitest';
import { literalMatches, matchedCharacterIndexes } from './searchHighlights';

describe('search highlight ranges', () => {
  it('keeps the search panel’s literal case rules, including regular-expression characters', () => {
    expect(literalMatches('x NEEDLE.* needle.*', 'needle.*', false)).toEqual([{ start: 2, end: 10 }, { start: 11, end: 19 }]);
    expect(literalMatches('NEEDLE.* needle.*', 'needle.*', true)).toEqual([{ start: 9, end: 17 }]);
    expect(literalMatches('needle needle needle', 'needle', true, 1)).toEqual([{ start: 0, end: 6 }]);
  });

  it('maps repeated, cross-glyph Unicode matches through whitespace without splitting code units', () => {
    const characters = [{ text: 'A' }, { text: ' ' }, { text: 'B' }, { text: '\n' }, { text: 'é' }, { text: '🙂' }, { text: 'A' }, { text: ' ' }, { text: 'B' }];
    expect([...matchedCharacterIndexes(characters, { query: 'A B', matchCase: true })]).toEqual([0, 1, 2, 6, 7, 8]);
    expect([...matchedCharacterIndexes(characters, { query: 'B\né🙂A', matchCase: true })]).toEqual([2, 3, 4, 5, 6]);
  });
});
