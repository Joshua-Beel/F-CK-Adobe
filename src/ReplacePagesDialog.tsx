import { useEffect, useRef, useState } from 'react';
import type { SavedCopy } from './bridge';
import type { DocumentInfo } from './model';
import s from './ReplacePagesDialog.module.css';

export const MAX_REPLACEMENT_INPUT_PAGES = 4096;
type Source = { id: number; revision: number };
type Preview = { target: DocumentInfo; donor: DocumentInfo; start: number; count: number; outputPages: number };

export function replacementPreview(documents: DocumentInfo[], targetId: number, donorId: number, firstPage: string, pageCount: string): Preview | string {
  if (targetId === donorId) return 'Choose two different open PDFs.';
  const target = documents.find(document => document.id === targetId);
  const donor = documents.find(document => document.id === donorId);
  if (!target || !donor) return 'One selected PDF is no longer open. Choose two open PDFs.';
  if (!/^\d+$/.test(firstPage.trim()) || !/^\d+$/.test(pageCount.trim())) return `Enter a first page from 1 to ${target.pages.length} and a positive whole-number count.`;
  const first = Number(firstPage), count = Number(pageCount);
  if (!Number.isSafeInteger(first) || !Number.isSafeInteger(count) || first < 1 || count < 1 || first > target.pages.length || count > target.pages.length - first + 1) return `Enter a first page from 1 to ${target.pages.length} and a positive whole-number count.`;
  if (target.pages.length + donor.pages.length > MAX_REPLACEMENT_INPUT_PAGES) return `These input PDFs have ${(target.pages.length + donor.pages.length).toLocaleString()} pages. Replace Pages supports at most ${MAX_REPLACEMENT_INPUT_PAGES.toLocaleString()} input pages.`;
  return { target, donor, start: first - 1, count, outputPages: target.pages.length - count + donor.pages.length };
}

const source = (document: DocumentInfo): Source => ({ id: document.id, revision: document.revision });
const pageRange = (start: number, count: number) => count === 1 ? `page ${start + 1}` : `pages ${start + 1}–${start + count}`;

export default function ReplacePagesDialog({ documents, activeId, initialRange, busy, replace, close }: { documents: DocumentInfo[]; activeId: number | null; initialRange: { start: number; count: number }; busy: boolean; replace: (target: Source, donor: Source, start: number, count: number) => Promise<SavedCopy | null>; close: () => void }) {
  const initialTarget = documents.find(document => document.id === activeId) ?? documents[0];
  const initialDonor = documents.find(document => document.id !== initialTarget?.id) ?? initialTarget;
  const [targetId, setTargetId] = useState(initialTarget?.id ?? -1);
  const [donorId, setDonorId] = useState(initialDonor?.id ?? -1);
  const [firstPage, setFirstPage] = useState(String(initialRange.start + 1));
  const [pageCount, setPageCount] = useState(String(initialRange.count));
  const [error, setError] = useState('');
  const [submitting, setSubmitting] = useState(false);
  const dialog = useRef<HTMLDialogElement>(null);
  const inFlight = useRef(false);
  useEffect(() => { dialog.current?.showModal(); }, []);
  const preview = replacementPreview(documents, targetId, donorId, firstPage, pageCount);
  const working = busy || submitting;
  const submit = async () => {
    if (working || inFlight.current) return;
    if (typeof preview === 'string') { setError(preview); return; }
    inFlight.current = true; setSubmitting(true); setError('');
    try { await replace(source(preview.target), source(preview.donor), preview.start, preview.count); close(); }
    catch (reason) { setError(String(reason)); }
    finally { inFlight.current = false; setSubmitting(false); }
  };
  const targetPages = documents.find(document => document.id === targetId)?.pages.length ?? 0;
  return <dialog ref={dialog} className={s.dialog} aria-labelledby="replace-pages-title" onCancel={event => { event.preventDefault(); if (!working) close(); }}>
    <h2 id="replace-pages-title">Replace Pages in a New Copy</h2>
    <p>Choose a target PDF, another open donor PDF, and one contiguous target range. Windows will ask where to save the new PDF.</p>
    <label>Target PDF <select aria-label="Target PDF" value={targetId} disabled={working} onChange={event => { setTargetId(Number(event.target.value)); setFirstPage('1'); setPageCount('1'); setError(''); }}>{documents.map(document => <option key={document.id} value={document.id}>{document.name} ({document.pages.length} pages)</option>)}</select></label>
    <label>Replacement pages <select aria-label="Donor PDF" value={donorId} disabled={working} onChange={event => { setDonorId(Number(event.target.value)); setError(''); }}>{documents.map(document => <option key={document.id} value={document.id}>{document.name} ({document.pages.length} pages)</option>)}</select></label>
    <label>First target page <input aria-label="First target page" inputMode="numeric" value={firstPage} disabled={working} onChange={event => { setFirstPage(event.target.value); setError(''); }} /></label>
    <label>Target pages to replace <input aria-label="Target pages to replace" inputMode="numeric" value={pageCount} disabled={working} onChange={event => { setPageCount(event.target.value); setError(''); }} /></label>
    <p className={s.preview}>{typeof preview === 'string' ? preview : `${preview.donor.name} (${preview.donor.pages.length} pages) replaces ${pageRange(preview.start, preview.count)} in ${preview.target.name} (${preview.target.pages.length} pages): ${preview.outputPages} pages.`} The range must stay within pages 1–{targetPages}.</p>
    <p>The target PDF’s document metadata is copied as-is, even when every target page is replaced. Metadata is not merged or invented, and both source PDFs stay unchanged.</p>
    {error && <p role="alert">{error}</p>}
    {working && <p role="status">Choosing an output location and validating the new PDF…</p>}
    <div className={s.actions}><button disabled={working} onClick={close}>Cancel</button><button className={s.confirm} disabled={working} onClick={() => void submit()}>Replace pages</button></div>
  </dialog>;
}
