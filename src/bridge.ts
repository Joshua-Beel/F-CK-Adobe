import { invoke, isTauri } from '@tauri-apps/api/core';
import type { DocumentInfo } from './model';
export const native = isTauri();
export async function openDocument(example = false) {
  if (!native) throw new Error('Open the Windows desktop application to load a PDF. This browser view previews the workspace only.');
  return invoke<DocumentInfo | null>(example ? 'open_example' : 'open_document');
}
export const closeDocument = (id: number) => invoke<void>('close_document', { id });
export async function renderPage(id: number, page: number, width: number) {
  const bytes = await invoke<ArrayBuffer>('render_page', { id, page, width });
  return URL.createObjectURL(new Blob([bytes], { type: 'image/png' }));
}
