import { describe, expect, it } from 'vitest';
import { clampPage, pageOffsets, visiblePages } from './model';
describe('document viewport', () => {
  it('bounds invalid navigation', () => { expect(clampPage(-1, 98)).toBe(0); expect(clampPage(120, 98)).toBe(97); expect(clampPage(NaN, 98)).toBe(0); });
  it('lays out mixed page sizes without overlap', () => { expect(pageOffsets([{ width: 612, height: 792 }, { width: 792, height: 612 }], 2)).toEqual([24, 1632]); });
  it('keeps raster work bounded on a 1500-page document', () => {
    const pages = Array.from({ length: 1500 }, () => ({ width: 612, height: 792 }));
    const offsets = pageOffsets(pages, 1);
    const visible = visiblePages(offsets, pages, 1, offsets[749], 1000);
    expect(visible).toContain(749); expect(visible.length).toBeLessThanOrEqual(5); expect(visible).not.toContain(0);
  });
});
