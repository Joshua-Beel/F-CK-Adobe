import { useEffect, useRef, useState } from 'react';
import type { SavedCopy } from './bridge';
import type { DocumentInfo } from './model';
import s from './InsertPagesDialog.module.css';

export const MAX_INSERTED_PAGES = 4096;
type Source = { id: number; revision: number };
type Preview = { target: DocumentInfo; donor: DocumentInfo; at: number; pages: number };

export function insertionPreview(documents: DocumentInfo[], targetId: number, donorId: number, boundary: string): Preview | string {
  if (targetId === donorId) return 'Choose two different open PDFs.';
  const target = documents.find(document => document.id === targetId);
  const donor = documents.find(document => document.id === donorId);
  if (!target || !donor) return 'One selected PDF is no longer open. Choose two open PDFs.';
  if (!/^\d+$/.test(boundary.trim())) return `Enter a whole-number insertion boundary from 0 to ${target.pages.length}.`;
  const at = Number(boundary);
  if (!Number.isSafeInteger(at) || at < 0 || at > target.pages.length) return `Enter a whole-number insertion boundary from 0 to ${target.pages.length}.`;
  const pages = target.pages.length + donor.pages.length;
  if (pages > MAX_INSERTED_PAGES) return `These PDFs have ${pages.toLocaleString()} pages. Insert Pages supports at most ${MAX_INSERTED_PAGES.toLocaleString()} pages.`;
  return { target, donor, at, pages };
}

const source = (document: DocumentInfo): Source => ({ id: document.id, revision: document.revision });
const position = (at: number, pages: number) => at === 0 ? 'before page 1' : at === pages ? 'after the last page (append)' : `after page ${at}`;

export default function InsertPagesDialog({ documents, activeId, busy, insert, close }: { documents: DocumentInfo[]; activeId: number | null; busy: boolean; insert: (target: Source, donor: Source, at: number) => Promise<SavedCopy | null>; close: () => void }) {
  const initialTarget = documents.find(document => document.id === activeId) ?? documents[0];
  const initialDonor = documents.find(document => document.id !== initialTarget?.id) ?? initialTarget;
  const [targetId, setTargetId] = useState(initialTarget?.id ?? -1);
  const [donorId, setDonorId] = useState(initialDonor?.id ?? -1);
  const [boundary, setBoundary] = useState('0');
  const [error, setError] = useState('');
  const [submitting, setSubmitting] = useState(false);
  const dialog = useRef<HTMLDialogElement>(null);
  const inFlight = useRef(false);
  useEffect(() => { dialog.current?.showModal(); }, []);
  const preview = insertionPreview(documents, targetId, donorId, boundary);
  const working = busy || submitting;
  const select = (setter: (id: number) => void, value: string) => { setter(Number(value)); setError(''); };
  const submit = async () => {
    if (working || inFlight.current) return;
    if (typeof preview === 'string') { setError(preview); return; }
    inFlight.current = true; setSubmitting(true); setError('');
    try { await insert(source(preview.target), source(preview.donor), preview.at); close(); }
    catch (reason) { setError(String(reason)); }
    finally { inFlight.current = false; setSubmitting(false); }
  };
  const targetPages = documents.find(document => document.id === targetId)?.pages.length ?? 0;
  return <dialog ref={dialog} className={s.dialog} aria-labelledby="insert-pages-title" onCancel={event => { event.preventDefault(); if (!working) close(); }}>
    <h2 id="insert-pages-title">Insert Pages into a New Copy</h2>
    <p>Choose a target PDF, another open donor PDF, and where the donor’s current edited pages belong. Windows will ask where to save the new PDF.</p>
    <label>Target PDF <select aria-label="Target PDF" value={targetId} disabled={working} onChange={event => { select(setTargetId, event.target.value); setBoundary('0'); }}>{documents.map(document => <option key={document.id} value={document.id}>{document.name} ({document.pages.length} pages)</option>)}</select></label>
    <label>Pages to insert <select aria-label="Donor PDF" value={donorId} disabled={working} onChange={event => select(setDonorId, event.target.value)}>{documents.map(document => <option key={document.id} value={document.id}>{document.name} ({document.pages.length} pages)</option>)}</select></label>
    <label>Insertion boundary <input aria-label="Insertion boundary" inputMode="numeric" value={boundary} disabled={working} onChange={event => { setBoundary(event.target.value); setError(''); }} /></label>
    <p className={s.preview}>{typeof preview === 'string' ? preview : `${preview.donor.name} (${preview.donor.pages.length} pages) goes ${position(preview.at, preview.target.pages.length)} in ${preview.target.name} (${preview.target.pages.length} pages): ${preview.pages} pages.`} Boundaries run from 0 before the first page to {targetPages} after the last page.</p>
    <p>The target PDF’s document metadata is copied as-is. Metadata is not merged or invented, and both source PDFs stay unchanged.</p>
    {error && <p role="alert">{error}</p>}
    {working && <p role="status">Choosing an output location and validating the new PDF…</p>}
    <div className={s.actions}><button disabled={working} onClick={close}>Cancel</button><button className={s.confirm} disabled={working} onClick={() => void submit()}>Insert pages</button></div>
  </dialog>;
}
