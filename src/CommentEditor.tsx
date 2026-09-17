import { useEffect, useRef, useState } from 'react';
import type { CommentNote, CommentRect } from './bridge';
import s from './CommentEditor.module.css';

export type CommentDraft = { kind: 'create'; page: number; rect: CommentRect } | { kind: 'edit'; note: CommentNote };
const MAX_COMMENT_BYTES = 8 * 1024;

export function validateCommentContents(contents: string): string | null {
  if (!contents.trim()) return 'Enter comment text.';
  if (contents.includes('\0')) return 'Comment text cannot contain NUL characters.';
  if (new TextEncoder().encode(contents).byteLength > MAX_COMMENT_BYTES) return 'Comment text must be 8 KiB or less.';
  return null;
}

export default function CommentEditor({ draft, busy, save, remove, close }: { draft: CommentDraft; busy: boolean; save: (contents: string) => Promise<void>; remove?: () => Promise<void>; close: () => void }) {
  const dialog = useRef<HTMLDialogElement>(null);
  const [contents, setContents] = useState(draft.kind === 'edit' ? draft.note.contents : '');
  const [error, setError] = useState('');
  const [submitting, setSubmitting] = useState(false);
  const inFlight = useRef(false);
  useEffect(() => { dialog.current?.showModal(); }, []);
  const working = busy || submitting;
  const submit = async () => {
    if (working || inFlight.current) return;
    const problem = validateCommentContents(contents);
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
  const page = draft.kind === 'edit' ? draft.note.page : draft.page;
  return <dialog ref={dialog} className={s.dialog} aria-labelledby="comment-title" onCancel={event => { event.preventDefault(); if (!working) close(); }}>
    <h2 id="comment-title">{draft.kind === 'edit' ? `Comment on page ${page + 1}` : `New comment on page ${page + 1}`}</h2>
    <label>Comment<textarea aria-label="Comment text" autoFocus value={contents} disabled={working} maxLength={MAX_COMMENT_BYTES} onChange={event => { setContents(event.target.value); setError(''); }} /></label>
    <p>{new TextEncoder().encode(contents).byteLength.toLocaleString()} of {MAX_COMMENT_BYTES.toLocaleString()} bytes</p>
    {error && <p role="alert">{error}</p>}
    {working && <p role="status">Saving comment…</p>}
    <div className={s.actions}>{remove && <button className={s.delete} disabled={working} onClick={() => void destroy()}>Delete comment</button>}<span /><button disabled={working} onClick={close}>Cancel</button><button className={s.confirm} disabled={working} onClick={() => void submit()}>Save comment</button></div>
  </dialog>;
}
