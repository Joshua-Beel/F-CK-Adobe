import { useEffect, useRef, useState } from 'react';
import type { SavedCopy } from './bridge';
import type { DocumentInfo } from './model';
import s from './CombineDialog.module.css';

export const MAX_COMBINED_PAGES = 4096;
type Source = { id: number; revision: number };
type Preview = { first: DocumentInfo; second: DocumentInfo; pages: number };

export function combinePreview(documents: DocumentInfo[], firstId: number, secondId: number): Preview | string {
  if (firstId === secondId) return 'Choose two different open PDFs.';
  const first = documents.find(document => document.id === firstId);
  const second = documents.find(document => document.id === secondId);
  if (!first || !second) return 'One selected PDF is no longer open. Choose two open PDFs.';
  const pages = first.pages.length + second.pages.length;
  if (pages > MAX_COMBINED_PAGES) return `These PDFs have ${pages.toLocaleString()} pages. Combine supports at most ${MAX_COMBINED_PAGES.toLocaleString()} pages.`;
  return { first, second, pages };
}

const source = (document: DocumentInfo): Source => ({ id: document.id, revision: document.revision });

export default function CombineDialog({ documents, activeId, busy, combine, close }: { documents: DocumentInfo[]; activeId: number | null; busy: boolean; combine: (first: Source, second: Source) => Promise<SavedCopy | null>; close: () => void }) {
  const initialFirst = documents.find(document => document.id === activeId) ?? documents[0];
  const initialSecond = documents.find(document => document.id !== initialFirst?.id) ?? initialFirst;
  const [firstId, setFirstId] = useState(initialFirst?.id ?? -1);
  const [secondId, setSecondId] = useState(initialSecond?.id ?? -1);
  const [error, setError] = useState('');
  const [submitting, setSubmitting] = useState(false);
  const dialog = useRef<HTMLDialogElement>(null);
  const inFlight = useRef(false);
  useEffect(() => { dialog.current?.showModal(); }, []);
  const preview = combinePreview(documents, firstId, secondId);
  const working = busy || submitting;
  const select = (setter: (id: number) => void, value: string) => { setter(Number(value)); setError(''); };
  const submit = async () => {
    if (working || inFlight.current) return;
    if (typeof preview === 'string') { setError(preview); return; }
    inFlight.current = true; setSubmitting(true); setError('');
    try { await combine(source(preview.first), source(preview.second)); close(); }
    catch (reason) { setError(String(reason)); }
    finally { inFlight.current = false; setSubmitting(false); }
  };
  return <dialog ref={dialog} className={s.dialog} aria-labelledby="combine-title" onCancel={event => { event.preventDefault(); if (!working) close(); }}>
    <h2 id="combine-title">Combine PDFs</h2>
    <p>Choose the order for the current edited pages. Windows will ask where to save the new PDF.</p>
    <label>First PDF <select aria-label="First PDF" value={firstId} disabled={working} onChange={event => select(setFirstId, event.target.value)}>{documents.map(document => <option key={document.id} value={document.id}>{document.name} ({document.pages.length} pages)</option>)}</select></label>
    <label>Second PDF <select aria-label="Second PDF" value={secondId} disabled={working} onChange={event => select(setSecondId, event.target.value)}>{documents.map(document => <option key={document.id} value={document.id}>{document.name} ({document.pages.length} pages)</option>)}</select></label>
    <p className={s.preview}>{typeof preview === 'string' ? preview : `${preview.first.name} (${preview.first.pages.length}), then ${preview.second.name} (${preview.second.pages.length}): ${preview.pages} pages.`}</p>
    <p>The first PDF’s document metadata is copied as-is. Metadata is not merged or invented.</p>
    {error && <p role="alert">{error}</p>}
    {working && <p role="status">Choosing an output location and validating the combined PDF…</p>}
    <div className={s.actions}><button disabled={working} onClick={close}>Cancel</button><button className={s.confirm} disabled={working} onClick={() => void submit()}>Combine PDFs</button></div>
  </dialog>;
}
