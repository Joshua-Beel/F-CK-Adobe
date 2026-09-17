import { useEffect, useRef, useState } from 'react';
import type { DocumentFormFields, FormField, SavedCopy } from './bridge';
import type { DocumentInfo } from './model';
import s from './FillFormsDialog.module.css';

type Patch = { fieldId: string; value: string };

export function formPatches(fields: FormField[], values: Record<string, string>): Patch[] {
  return fields.flatMap(field => {
    const value = values[field.fieldId] ?? field.value;
    return value === field.value ? [] : [{ fieldId: field.fieldId, value }];
  });
}

export function validateFormPatches(fields: FormField[], values: Record<string, string>, byteLimit: number): string | null {
  const patches = formPatches(fields, values);
  if (!patches.length) return 'Change at least one field before saving a new copy.';
  if (!Number.isSafeInteger(byteLimit) || byteLimit < 0) return 'This form has an invalid value limit.';
  for (const patch of patches) {
    const field = fields.find(item => item.fieldId === patch.fieldId);
    if (!field) return 'This form changed. Reload it and try again.';
    if (!/^[\x20-\x7E]*$/.test(patch.value)) return `${field.name} accepts printable ASCII characters only.`;
    if (new TextEncoder().encode(patch.value).byteLength > byteLimit) return `${field.name} exceeds the ${byteLimit.toLocaleString()}-byte value limit.`;
    if (field.maxLength !== null && patch.value.length > field.maxLength) return `${field.name} exceeds its ${field.maxLength.toLocaleString()}-character limit.`;
  }
  return null;
}

export default function FillFormsDialog({ document, formFields, error: loadError, busy, fill, close }: { document: DocumentInfo; formFields: DocumentFormFields | null; error: string; busy: boolean; fill: (id: number, revision: number, values: Patch[]) => Promise<SavedCopy | null>; close: () => void }) {
  const dialog = useRef<HTMLDialogElement>(null);
  const inFlight = useRef(false);
  const initialized = useRef('');
  const [values, setValues] = useState<Record<string, string>>({});
  const [error, setError] = useState('');
  const [notice, setNotice] = useState('');
  const [submitting, setSubmitting] = useState(false);
  useEffect(() => { dialog.current?.showModal(); }, []);
  const fields = formFields?.status === 'supported' ? formFields.fields : [];
  const identity = formFields ? `${formFields.documentId}:${formFields.revision}:${fields.map(field => field.fieldId).join(',')}` : '';
  useEffect(() => {
    if (!formFields || formFields.status !== 'supported' || initialized.current === identity) return;
    initialized.current = identity;
    setValues(Object.fromEntries(formFields.fields.map(field => [field.fieldId, field.value])));
  }, [formFields, identity]);
  const working = busy || submitting;
  const available = !!formFields && formFields.status === 'supported';
  const submit = async () => {
    if (working || inFlight.current || !formFields || formFields.status !== 'supported') return;
    const validation = validateFormPatches(fields, values, formFields.valueByteLimit);
    if (validation) { setError(validation); return; }
    inFlight.current = true; setSubmitting(true); setError(''); setNotice('');
    try {
      const result = await fill(document.id, document.revision, formPatches(fields, values));
      if (result) close();
      else setNotice('Saving was canceled. Your entries are still here.');
    } catch (reason) { setError(String(reason)); }
    finally { inFlight.current = false; setSubmitting(false); }
  };
  return <dialog ref={dialog} className={s.dialog} aria-labelledby="fill-forms-title" onCancel={event => { event.preventDefault(); if (!working) close(); }}>
    <h2 id="fill-forms-title">Fill existing text fields</h2>
    <p>Enter printable ASCII text for the supported fields below. Windows will ask where to save a new PDF. Your open source PDF stays unchanged.</p>
    {loadError ? <p role="alert">{loadError}</p> : !formFields ? <p role="status">Checking this PDF for supported text fields…</p> : formFields.status === 'unsupported' ? <p role="status">Filling is unavailable. {formFields.reason || 'This PDF has form fields this build does not change.'}</p> : !fields.length ? <p role="status">This PDF has no supported text fields.</p> : <div className={s.fields}>{fields.map(field => <label key={field.fieldId}><span>{field.name}<small>Page {field.page + 1}{field.maxLength === null ? '' : ` · ${field.maxLength} characters`}</small></span><input aria-label={field.name} value={values[field.fieldId] ?? field.value} disabled={working} maxLength={field.maxLength ?? undefined} onChange={event => { setValues(current => ({ ...current, [field.fieldId]: event.target.value })); setError(''); setNotice(''); }} /></label>)}</div>}
    {error && <p role="alert">{error}</p>}
    {notice && <p role="status">{notice}</p>}
    {working && <p role="status">Choosing an output location and validating the filled PDF…</p>}
    <div className={s.actions}><button disabled={working} onClick={close}>Cancel</button><button className={s.confirm} disabled={working || !available || !fields.length} onClick={() => void submit()}>Save filled copy</button></div>
  </dialog>;
}
