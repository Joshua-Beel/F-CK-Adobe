export type PageSize = { width: number; height: number };
export type DocumentInfo = { id: number; name: string; path: string; pages: PageSize[]; revision: number; dirty: boolean; can_undo: boolean; can_redo: boolean };
export type PageEdit = { kind: 'rotate'; pages: number[]; clockwise: boolean } | { kind: 'delete'; pages: number[] } | { kind: 'move'; from: number; to: number } | { kind: 'undo' | 'redo' };
export function parsePageRange(text: string, count: number): number[] {
  const pages = new Set<number>();
  if (!text.trim()) throw new Error('Enter page numbers, such as 1-3, 5.');
  for (const part of text.split(',')) {
    const match = part.trim().match(/^(\d+)(?:\s*-\s*(\d+))?$/);
    if (!match) throw new Error('Use page numbers and ranges, such as 1-3, 5.');
    const first = Number(match[1]), last = Number(match[2] || match[1]);
    if (first < 1 || last > count || last < first) throw new Error(`Choose pages between 1 and ${count}.`);
    for (let i = first; i <= last; i++) pages.add(i - 1);
  }
  return [...pages].sort((a, b) => a - b);
}
export const clampPage = (value: number, count: number) => Math.max(0, Math.min(count - 1, Number.isFinite(value) ? Math.trunc(value) : 0));
export function pageOffsets(pages: PageSize[], scale: number) {
  let offset = 24;
  return pages.map(page => { const top = offset; offset += page.height * scale + 24; return top; });
}
export function visiblePages(offsets: number[], pages: PageSize[], scale: number, top: number, height: number) {
  return pages.map((_, i) => i).filter(i => offsets[i] + pages[i].height * scale >= top - height / 2 && offsets[i] <= top + height * 1.5);
}
export const toolGroups = [
  { name: 'Create & edit', tools: ['Create a PDF', 'Combine files', 'Organize pages', 'Edit a PDF', 'Export a PDF', 'Scan & OCR', 'Rich media'] },
  { name: 'Review', tools: ['Comment', 'Add a stamp', 'Compare files', 'Measure', 'Export package', 'Comment round-trip'] },
  { name: 'Forms & signatures', tools: ['Fill forms', 'Prepare a form', 'Use a certificate'] },
  { name: 'Protect & standardize', tools: ['Protect a PDF', 'Redact a PDF', 'PDF standards', 'Compress a PDF', 'Print preview', 'Prepare for accessibility'] },
  { name: 'Customize', tools: ['Create custom tool', 'Use guided actions', 'Add search index'] }
];
