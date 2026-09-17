import { useEffect, useRef, useState } from 'react';
import type { Annotation, CommentRect } from './bridge';
import s from './CommentEditor.module.css';

export type AnnotationDraft = { kind: 'create'; type: 'note' | 'area-highlight'; page: number; rect: CommentRect } | { kind: 'create-text-highlight'; page: number; start: number; end: number } | { kind: 'edit'; annotation: Annotation };
const MAX_COMMENT_BYTES = 8 * 1024;

export function validateCommentContents(contents: string): string | null {
  if (!contents.trim()) return 'Enter comment text.';
  if (contents.includes('\0')) return 'Comment text cannot contain NUL characters.';
  if (new TextEncoder().encode(contents).byteLength > MAX_COMMENT_BYTES) return 'Comment text must be 8 KiB or less.';
  return null;
}

function validateOptionalContents(contents: string): string | null {
  if (contents.includes('\0')) return 'Comment text cannot contain NUL characters.';
  if (new TextEncoder().encode(contents).byteLength > MAX_COMMENT_BYTES) return 'Comment text must be 8 KiB or less.';
  return null;
}

export default function CommentEditor({ draft, busy, save, remove, close }: { draft: AnnotationDraft; busy: boolean; save: (contents: string | null) => Promise<void>; remove?: () => Promise<void>; close: () => void }) {
  const dialog = useRef<HTMLDialogElement>(null);
  const type = draft.kind === 'edit' ? draft.annotation.kind : draft.kind === 'create-text-highlight' ? 'text-highlight' : draft.type;
  const required = type === 'note';
  const [contents, setContents] = useState(draft.kind === 'edit' ? draft.annotation.contents || '' : '');
  const [error, setError] = useState('');
  const [submitting, setSubmitting] = useState(false);
  const inFlight = useRef(false);
  useEffect(() => { dialog.current?.showModal(); }, []);
  const working = busy || submitting;
  const submit = async () => {
    if (working || inFlight.current) return;
    const problem = required ? validateCommentContents(contents) : validateOptionalContents(contents);
    if (problem) { setError(problem); return; }
    inFlight.current = true; setSubmitting(true); setError('');
    try { await save(contents); close(); }
    catch (reason) { setError(String(reason)); }
    finally { inFlight.current = false; setSubmitting(false); }
  };
  const destroy = async () => {
    if (!remove || working || inFlight.current) return;
    inFlight.current = true; setSubmitting(true); setError('');
    try { await remove(); close(); }
    catch (reason) { setError(String(reason)); }
    finally { inFlight.current = false; setSubmitting(false); }
  };
  const page = draft.kind === 'edit' ? draft.annotation.page : draft.page;
  const noun = type === 'note' ? 'Comment' : type === 'text-highlight' ? 'Text highlight' : type === 'area-highlight' ? 'Area highlight' : 'Highlight';
  return <dialog ref={dialog} className={s.dialog} aria-labelledby="comment-title" onCancel={event => { event.preventDefault(); if (!working) close(); }}>
    <h2 id="comment-title">{draft.kind === 'edit' ? `${noun} on page ${page + 1}` : `New ${noun.toLowerCase()} on page ${page + 1}`}</h2>
    <label>{required ? 'Comment' : 'Description (optional)'}<textarea aria-label={required ? 'Comment text' : 'Highlight description'} autoFocus value={contents} disabled={working} maxLength={MAX_COMMENT_BYTES} onChange={event => { setContents(event.target.value); setError(''); }} /></label>
    <p>{new TextEncoder().encode(contents).byteLength.toLocaleString()} of {MAX_COMMENT_BYTES.toLocaleString()} bytes</p>
    {error && <p role="alert">{error}</p>}
    {working && <p role="status">Saving comment…</p>}
    <div className={s.actions}>{remove && <button className={s.delete} disabled={working} onClick={() => void destroy()}>Delete {noun.toLowerCase()}</button>}<span /><button disabled={working} onClick={close}>Cancel</button><button className={s.confirm} disabled={working} onClick={() => void submit()}>Save {noun.toLowerCase()}</button></div>
  </dialog>;
}
