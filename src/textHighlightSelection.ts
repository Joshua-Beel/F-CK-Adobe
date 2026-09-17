export type TextHighlightRange = { start: number; end: number };
export type TextHighlightSelection = TextHighlightRange & { id: number; page: number; revision: number };
export type TextHighlightSelectionSource = Pick<TextHighlightSelection, 'id' | 'page' | 'revision'>;

export type GlyphEndpoint = { index: number; offset: number; length: number };

export function wholeGlyphRange(first: GlyphEndpoint, second: GlyphEndpoint): TextHighlightRange | null {
  const boundary = (endpoint: GlyphEndpoint, side: 'start' | 'end') => {
    if (!Number.isSafeInteger(endpoint.index) || !Number.isSafeInteger(endpoint.offset) || !Number.isSafeInteger(endpoint.length) || endpoint.index < 0 || endpoint.length < 0 || endpoint.offset < 0 || endpoint.offset > endpoint.length) return null;
    if (endpoint.offset === 0) return endpoint.index;
    if (endpoint.offset === endpoint.length) return endpoint.index + 1;
    return side === 'start' ? endpoint.index : endpoint.index + 1;
  };
  const start = boundary(first, 'start'), end = boundary(second, 'end');
  if (start === null || end === null) return null;
  const range = { start: Math.min(start, end), end: Math.max(start, end) };
  return range.start < range.end ? range : null;
}

function indexOf(element: HTMLElement) {
  const value = Number(element.dataset.geometryIndex);
  return Number.isSafeInteger(value) && value >= 0 ? value : null;
}

function mappedParent(node: Node, layer: HTMLElement) {
  if (!layer.contains(node)) return null;
  let current: HTMLElement | null = node.nodeType === Node.ELEMENT_NODE ? node as HTMLElement : node.parentElement;
  while (current && current !== layer) {
    if (current.dataset.geometryIndex !== undefined) return layer.contains(current) ? current : null;
    current = current.parentElement;
  }
  return null;
}

function endpointOffset(node: Node, offset: number, glyph: HTMLElement) {
  const text = glyph.firstChild;
  if (node === glyph) return offset === 0 ? 0 : glyph.textContent?.length ?? 0;
  if (node === text && text?.nodeType === Node.TEXT_NODE) return offset;
  return null;
}

function endpoint(node: Node, offset: number, layer: HTMLElement): GlyphEndpoint | null {
  if (node === layer) {
    const children = [...layer.children] as HTMLElement[];
    if (!Number.isSafeInteger(offset) || offset < 0 || offset > children.length) return null;
    if (!children.length) return null;
    const child = offset === children.length ? children[children.length - 1] : children[offset];
    const index = indexOf(child);
    if (index === null) return null;
    return { index: offset === children.length ? index : index, offset: offset === children.length ? child.textContent?.length ?? 0 : 0, length: child.textContent?.length ?? 0 };
  }
  const glyph = mappedParent(node, layer);
  if (!glyph) return null;
  const index = indexOf(glyph), value = endpointOffset(node, offset, glyph), length = glyph.textContent?.length ?? 0;
  return index === null || value === null ? null : { index, offset: value, length };
}

export function textHighlightFromRange(range: Range, layer: HTMLElement, context: { id: number; page: number; revision: number }): TextHighlightSelection | null {
  if (!layer.isConnected || range.collapsed || layer.dataset.textLayer !== `${context.id}:${context.page}:${context.revision}`) return null;
  const first = endpoint(range.startContainer, range.startOffset, layer);
  const second = endpoint(range.endContainer, range.endOffset, layer);
  if (!first || !second) return null;
  const selection = wholeGlyphRange(first, second);
  return selection ? { ...context, ...selection } : null;
}

export function textHighlightFromBrowserSelection(selection: Selection | null, layer: HTMLElement, context: { id: number; page: number; revision: number }): TextHighlightSelection | null {
  if (!selection || selection.rangeCount !== 1) return null;
  return textHighlightFromRange(selection.getRangeAt(0), layer, context);
}
