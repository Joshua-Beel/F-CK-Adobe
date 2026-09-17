import { useEffect, useRef, useState } from 'react';
import s from './CropDialog.module.css';

type CropRect = { x: number; y: number; width: number; height: number };
type Insets = { top: string; right: string; bottom: string; left: string };
const emptyInsets: Insets = { top: '0', right: '0', bottom: '0', left: '0' };

function readInset(value: string) {
  const trimmed = value.trim();
  if (!trimmed) return null;
  const number = Number(trimmed);
  return Number.isFinite(number) && number >= 0 ? number : null;
}

export function cropPreview(pageWidth: number, pageHeight: number, insets: Insets): { rect: CropRect; width: number; height: number } | string {
  const top = readInset(insets.top), right = readInset(insets.right), bottom = readInset(insets.bottom), left = readInset(insets.left);
  if ([top, right, bottom, left].some(value => value === null)) return 'Enter finite, non-negative inset values in points.';
  const width = pageWidth - left! - right!, height = pageHeight - top! - bottom!;
  if (width < 1 || height < 1) return 'Insets must leave at least 1 point in both dimensions.';
  return { rect: { x: left! / pageWidth, y: top! / pageHeight, width: width / pageWidth, height: height / pageHeight }, width, height };
}

export default function CropDialog({ page, pageWidth, pageHeight, busy, crop, close }: { page: number; pageWidth: number; pageHeight: number; busy: boolean; crop: (rect: CropRect) => Promise<void>; close: () => void }) {
  const dialog = useRef<HTMLDialogElement>(null);
  const inFlight = useRef(false);
  const [insets, setInsets] = useState<Insets>(emptyInsets);
  const [error, setError] = useState('');
  const [submitting, setSubmitting] = useState(false);
  useEffect(() => { dialog.current?.showModal(); }, []);
  const preview = cropPreview(pageWidth, pageHeight, insets);
  const hasInsets = Object.values(insets).some(value => Number(value) > 0);
  const working = busy || submitting;
  const updateInset = (name: keyof Insets, value: string) => { setInsets(current => ({ ...current, [name]: value })); setError(''); };
  const submit = async () => {
    if (working || inFlight.current) return;
    if (typeof preview === 'string') { setError(preview); return; }
    if (!hasInsets) { setError('Enter an inset to crop this page.'); return; }
    inFlight.current = true; setSubmitting(true); setError('');
    try { await crop(preview.rect); close(); }
    catch (reason) { setError(String(reason)); }
    finally { inFlight.current = false; setSubmitting(false); }
  };
  return <dialog ref={dialog} className={s.dialog} aria-labelledby="crop-title" onCancel={event => { event.preventDefault(); if (!working) close(); }}>
    <h2 id="crop-title">Crop page {page + 1}</h2>
    <p>Enter the amount to hide from each edge in points. Cropping hides content; it is not redaction.</p>
    <div className={s.insets}>
      {(['top', 'right', 'bottom', 'left'] as const).map(name => <label key={name}>{name[0].toUpperCase() + name.slice(1)} <input aria-label={`${name[0].toUpperCase() + name.slice(1)} crop inset`} inputMode="decimal" value={insets[name]} disabled={working} onChange={event => updateInset(name, event.target.value)} onKeyDown={event => { if (event.key === 'Enter') { event.preventDefault(); void submit(); } }} /> <span>pt</span></label>)}
    </div>
    <p className={s.preview}>{typeof preview === 'string' ? preview : `Result: ${preview.width.toFixed(1)} × ${preview.height.toFixed(1)} pt from ${pageWidth.toFixed(1)} × ${pageHeight.toFixed(1)} pt.`}</p>
    {error && <p role="alert">{error}</p>}
    {working && <p role="status">Applying crop…</p>}
    <div className={s.actions}><button disabled={working} onClick={close}>Cancel</button><button className={s.confirm} disabled={working} onClick={() => void submit()}>Apply crop</button></div>
  </dialog>;
}
