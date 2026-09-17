import { useEffect, useRef, useState } from 'react';
import { pageText } from './bridge';
import type { DocumentInfo } from './model';
import s from './PageText.module.css';

export default function PageText({ document, page, close }: { document: DocumentInfo; page: number; close: () => void }) {
  const dialog = useRef<HTMLDialogElement>(null);
  const textArea = useRef<HTMLTextAreaElement>(null);
  const [text, setText] = useState<string | null>(null);
  const [error, setError] = useState('');
  useEffect(() => {
    dialog.current?.showModal();
    let disposed = false;
    setText(null); setError('');
    pageText(document.id, page, document.revision).then(value => { if (!disposed) setText(value); }).catch(e => { if (!disposed) setError(String(e)); });
    return () => { disposed = true; };
  }, [document.id, document.revision, page]);
  return <dialog ref={dialog} className={s.dialog} aria-labelledby="page-text-title" onCancel={event => { event.preventDefault(); close(); }}>
    <h2 id="page-text-title">Text on page {page + 1}</h2>
    <p>Select text and press Ctrl+C to copy. Reading order depends on the PDF.</p>
    {error ? <p role="alert">{error}</p> : text === null ? <p role="status">Reading page text…</p> : !text.trim() ? <p role="status">This page has no embedded text. Scanned images need OCR first.</p> : <textarea ref={textArea} aria-label={`Text on page ${page + 1}`} readOnly value={text} spellCheck={false} />}
    <div className={s.actions}><button disabled={!text?.trim()} onClick={() => { textArea.current?.focus(); textArea.current?.select(); }}>Select all text</button><button autoFocus onClick={close}>Close</button></div>
  </dialog>;
}
