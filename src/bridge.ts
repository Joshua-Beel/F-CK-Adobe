import { invoke, isTauri } from '@tauri-apps/api/core';
import type { DocumentInfo, PageEdit } from './model';
export const native = isTauri();
export type PasswordChallenge = { status: 'password_required'; request_id: number; name: string; incorrect: boolean };
export type OpenResult = { status: 'opened'; document: DocumentInfo } | PasswordChallenge;
export async function openDocument(example = false): Promise<OpenResult | null> {
  if (!native) throw new Error('Open the Windows desktop application to load a PDF. This browser view previews the workspace only.');
  return example ? { status: 'opened', document: await invoke<DocumentInfo>('open_example') } : invoke<OpenResult | null>('open_document');
}
export const closeDocument = (id: number) => invoke<void>('close_document', { id });
export const reopenDocument = (path: string) => invoke<OpenResult>('reopen_document', { path });
export const unlockDocument = (requestId: number, password: string) => invoke<OpenResult>('unlock_document', { requestId, password });
export const cancelPasswordRequest = (requestId: number) => invoke<void>('cancel_password_request', { requestId });
export const pageText = (id: number, page: number, revision: number) => invoke<string>('page_text', { id, page, revision });
export type BookmarkList = { items: { title: string; page: number | null; depth: number }[]; truncated: boolean };
export const documentBookmarks = (id: number, revision: number) => invoke<BookmarkList>('document_bookmarks', { id, revision });
export const editPages = (id: number, edit: PageEdit) => invoke<DocumentInfo>('edit_pages', { id, edit });
export const saveCopy = (id: number, pages?: number[]) => invoke<{ path: string; document: DocumentInfo } | null>('save_copy', { id, pages: pages ?? null });
export async function renderPage(id: number, page: number, width: number) {
  const bytes = await invoke<ArrayBuffer>('render_page', { id, page, width });
  return URL.createObjectURL(new Blob([bytes], { type: 'image/png' }));
}
