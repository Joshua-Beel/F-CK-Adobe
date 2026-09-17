import { useEffect, useRef } from 'react';
import s from './ConfirmDialog.module.css';
export default function ConfirmDialog({ title, message, confirmLabel, onConfirm, onCancel }: { title: string; message: string; confirmLabel: string; onConfirm: () => void; onCancel: () => void }) {
  const dialog = useRef<HTMLDialogElement>(null);
  useEffect(() => { dialog.current?.showModal(); }, []);
  return <dialog ref={dialog} className={s.dialog} onCancel={event => { event.preventDefault(); onCancel(); }} aria-labelledby="confirm-title"><h2 id="confirm-title">{title}</h2><p>{message}</p><div><button autoFocus onClick={onCancel}>Cancel</button><button className={s.confirm} onClick={onConfirm}>{confirmLabel}</button></div></dialog>;
}
