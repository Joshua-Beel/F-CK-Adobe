import { useEffect, useRef, useState } from 'react';
import { cancelPrint, printDocument } from './bridge';
import type { DocumentInfo } from './model';
import s from './PasswordDialog.module.css';

export default function PrintDialog({ document, page, setBusy, close }: { document: DocumentInfo; page: number; setBusy: (busy: boolean) => void; close: () => void }) {
  const dialog = useRef<HTMLDialogElement>(null);
  const request = useRef<string | null>(null);
  const [running, setRunning] = useState(false);
  const [status, setStatus] = useState('Choose a printer and page range in the Windows print dialog.');
  const [error, setError] = useState('');
  useEffect(() => { dialog.current?.showModal(); }, []);
  async function cancel() {
    const id = request.current;
    if (!id) { close(); return; }
    try { await cancelPrint(id); if (request.current === id) setStatus('Cancel requested. Pages already sent to the printer may still print.'); }
    catch (error) { if (request.current === id) setError(`Could not cancel printing: ${String(error)}`); }
  }
  async function print() {
    if (request.current) return;
    const id = crypto.randomUUID(); request.current = id;
    setRunning(true); setBusy(true); setError(''); setStatus('Waiting for the print dialog or sending pages to the printer…');
    try {
      const result = await printDocument(document.id, document.revision, page, id);
      setStatus(result.status === 'cancelled' ? 'Printing canceled. Pages already sent may still print; check the printer queue.' : `${result.pages} ${result.pages === 1 ? 'page' : 'pages'} submitted to the printer. Check the printer queue for completion.`);
    } catch (error) { setError(`Printing failed: ${String(error)}`); setStatus('Some pages may already have reached the printer. Check its queue before retrying.'); }
    finally { request.current = null; setRunning(false); setBusy(false); }
  }
  return <dialog ref={dialog} className={s.dialog} aria-labelledby="print-title" onCancel={event => { event.preventDefault(); void cancel(); }}>
    <h2 id="print-title">Print PDF</h2><p>{document.name}</p>
    <p>Pages are printed as images, fitted to the printable area. Your current page edits are included. Encrypted PDFs cannot be printed in this build.</p>
    <p role="status" aria-live="polite">{status}</p>{error && <p role="alert">{error}</p>}
    <div className={s.actions}><button onClick={() => void cancel()}>{running ? 'Cancel print' : 'Close'}</button><button disabled={running} onClick={() => void print()}>Choose printer…</button></div>
  </dialog>;
}
