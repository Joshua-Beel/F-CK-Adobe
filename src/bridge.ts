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
export type PrintResult = { status: 'cancelled' } | { status: 'submitted'; pages: number };
export const printDocument = (id: number, revision: number, currentPage: number, requestId: string) => invoke<PrintResult>('print_document', { id, revision, currentPage, requestId });
export const cancelPrint = (requestId: string) => invoke<void>('cancel_print', { requestId });
export type DocumentPropertiesInfo = {
  version: string | null; source_size_bytes: number; page_count: number;
  metadata: { name: string; value: string; truncated: boolean }[];
  page_dimensions: { width_points: number; height_points: number; count: number }[];
  page_dimensions_truncated: boolean;
  security: { encrypted: boolean | null; handler_revision: number | null; print_high_quality: boolean | null; print_low_quality_only: boolean | null; modify_contents: boolean | null; assemble_document: boolean | null; fill_existing_forms: boolean | null };
  reported_signature_count: number | null; signature_validation: 'not_performed';
};
export const documentProperties = (id: number, revision: number) => invoke<DocumentPropertiesInfo>('document_properties', { id, revision });
export const dependencyNotices = () => invoke<string>('dependency_notices');
export const pageText = (id: number, page: number, revision: number) => invoke<string>('page_text', { id, page, revision });
export type PageTextGeometry = {
  id: number; page: number; revision: number; status: 'ok' | 'unsupported'; truncated: boolean;
  characters: { text: string; bounds: { x: number; y: number; width: number; height: number } | null; angle: number }[];
  reason: string | null;
};
export const pageTextGeometry = (id: number, page: number, revision: number) => invoke<PageTextGeometry>('page_text_geometry', { id, page, revision });
export type BookmarkList = { items: { title: string; page: number | null; depth: number }[]; truncated: boolean };
export const documentBookmarks = (id: number, revision: number) => invoke<BookmarkList>('document_bookmarks', { id, revision });
export const editPages = (id: number, edit: PageEdit) => invoke<DocumentInfo>('edit_pages', { id, edit });
export type SavedCopy = { path: string; document: DocumentInfo };
export const saveCopy = (id: number, pages?: number[]) => invoke<SavedCopy | null>('save_copy', { id, pages: pages ?? null });
export const combineDocuments = (first: { id: number; revision: number }, second: { id: number; revision: number }) => invoke<SavedCopy | null>('combine_documents', { first, second });
export const insertPagesCopy = (target: { id: number; revision: number }, donor: { id: number; revision: number }, at: number) => invoke<SavedCopy | null>('insert_pages_copy', { target, donor, at });
export type SplitOutput = { folder: string; files: { path: string; first_page: number; last_page: number; page_count: number }[] };
export const splitDocument = (id: number, revision: number, pagesPerFile: number) => invoke<SplitOutput | null>('split_document', { id, revision, pagesPerFile });
export const cropPage = (id: number, page: number, revision: number, rect: { x: number; y: number; width: number; height: number }) => invoke<DocumentInfo>('crop_page', { id, page, revision, rect });
export async function renderPage(id: number, page: number, width: number) {
  const bytes = await invoke<ArrayBuffer>('render_page', { id, page, width });
  return URL.createObjectURL(new Blob([bytes], { type: 'image/png' }));
}
