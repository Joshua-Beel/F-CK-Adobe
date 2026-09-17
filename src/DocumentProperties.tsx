import { useEffect, useRef, useState } from 'react';
import { documentProperties, type DocumentPropertiesInfo } from './bridge';
import type { DocumentInfo } from './model';
import s from './DocumentProperties.module.css';

const metadataLabels: Record<string, string> = { title: 'Title', author: 'Author', subject: 'Subject', keywords: 'Keywords', creator: 'Created with', producer: 'PDF producer', creation_date_raw: 'Created (as stored)', modification_date_raw: 'Modified (as stored)' };
const permissionLabels = { print_high_quality: 'High-quality printing', print_low_quality_only: 'Low-quality printing only', modify_contents: 'Modify contents', assemble_document: 'Assemble pages', fill_existing_forms: 'Fill existing forms' };

export default function DocumentProperties({ document, close }: { document: DocumentInfo; close: () => void }) {
  const dialog = useRef<HTMLDialogElement>(null);
  const [info, setInfo] = useState<DocumentPropertiesInfo | null>(null);
  const [error, setError] = useState('');
  useEffect(() => {
    dialog.current?.showModal();
    let disposed = false;
    setInfo(null); setError('');
    documentProperties(document.id, document.revision).then(value => { if (!disposed) setInfo(value); }).catch(reason => { if (!disposed) setError(String(reason)); });
    return () => { disposed = true; };
  }, [document.id, document.revision]);
  return <dialog ref={dialog} className={s.dialog} aria-labelledby="properties-title" onCancel={event => { event.preventDefault(); close(); }}>
    <h2 id="properties-title">Document properties</h2>
    <p className={s.name}>{document.name}</p>
    {error ? <p role="alert">{error}</p> : info === null ? <p role="status">Reading document properties…</p> : <>
      <dl><dt>Location</dt><dd>{document.path}</dd><dt>Original file size</dt><dd>{info.source_size_bytes.toLocaleString()} bytes</dd><dt>PDF version</dt><dd>{info.version ?? 'Unknown'}</dd><dt>Current pages</dt><dd>{info.page_count.toLocaleString()}</dd><dt>Unsaved page edits</dt><dd>{document.dirty ? 'Yes' : 'No'}</dd></dl>
      <h3>Description</h3>
      {info.metadata.length ? <dl>{info.metadata.map(entry => <div className={s.row} key={entry.name}><dt>{metadataLabels[entry.name] ?? entry.name}</dt><dd>{entry.value}{entry.truncated && <span className={s.note}> (truncated)</span>}</dd></div>)}</dl> : <p>No description metadata was found.</p>}
      <h3>Page sizes</h3>
      <p>Current page sizes in points (72 points = 1 inch), including rotations.</p>
      <ul>{info.page_dimensions.map((size, index) => <li key={index}>{size.width_points.toLocaleString(undefined, { maximumFractionDigits: 2 })} × {size.height_points.toLocaleString(undefined, { maximumFractionDigits: 2 })} pt — {size.count} {size.count === 1 ? 'page' : 'pages'}</li>)}</ul>
      {info.page_dimensions_truncated && <p>Only the first 128 distinct page sizes are shown.</p>}
      <h3>Security</h3>
      <dl><dt>Encrypted</dt><dd>{info.security.encrypted === null ? 'Unknown' : info.security.encrypted ? 'Yes' : 'No'}</dd><dt>Security handler revision</dt><dd>{info.security.handler_revision ?? 'Not available'}</dd>{Object.entries(permissionLabels).map(([key, label]) => { const value = info.security[key as keyof typeof permissionLabels]; return <div className={s.row} key={key}><dt>{label}</dt><dd>{value === null ? 'Unknown' : value ? 'Yes' : 'No'}</dd></div>; })}<dt>Reported signatures</dt><dd>{info.reported_signature_count ?? 'Unknown'}</dd></dl>
      <p>Signature validity has not been checked. Reported permissions describe the PDF; available app tools may be more limited. Encrypted files remain read-only in this build.</p>
      <p>Metadata and file size describe the original file opened in this session. Page counts and sizes include current edits.</p>
    </>}
    <div className={s.actions}><button autoFocus onClick={close}>Close</button></div>
  </dialog>;
}
