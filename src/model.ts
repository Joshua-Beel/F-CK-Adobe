export type PageSize = { width: number; height: number };
export type DocumentInfo = { id: number; name: string; path: string; pages: PageSize[] };
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
  { name: 'Forms & signatures', tools: ['Fill & sign', 'Prepare a form', 'Use a certificate'] },
  { name: 'Protect & standardize', tools: ['Protect a PDF', 'Redact a PDF', 'PDF standards', 'Compress a PDF', 'Print preview', 'Prepare for accessibility'] },
  { name: 'Customize', tools: ['Create custom tool', 'Use guided actions', 'Add search index'] }
];
