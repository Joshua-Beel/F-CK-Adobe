export type SearchHighlightQuery = { query: string; matchCase: boolean };

export type TextMatch = { start: number; end: number };

function literalExpression(query: string, matchCase: boolean) {
  return new RegExp(query.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'), matchCase ? 'gu' : 'giu');
}

export function literalMatches(text: string, query: string, matchCase: boolean, maximum = Number.POSITIVE_INFINITY): TextMatch[] {
  const needle = query.trim();
  if (!needle) return [];
  const expression = literalExpression(needle, matchCase);
  const matches: TextMatch[] = [];
  for (let match = expression.exec(text); match; match = expression.exec(text)) {
    matches.push({ start: match.index, end: match.index + match[0].length });
    if (matches.length === maximum) return matches;
  }
  return matches;
}

export function matchedCharacterIndexes(characters: { text: string }[], query: SearchHighlightQuery): Set<number> {
  const text = characters.map(character => character.text).join('');
  const matches = literalMatches(text, query.query, query.matchCase);
  const indexes = new Set<number>();
  let start = 0;
  let matchIndex = 0;
  for (let index = 0; index < characters.length; index++) {
    const end = start + characters[index].text.length;
    while (matches[matchIndex] && matches[matchIndex].end <= start) matchIndex++;
    if (matches[matchIndex]?.start < end && matches[matchIndex].end > start) indexes.add(index);
    start = end;
  }
  return indexes;
}
