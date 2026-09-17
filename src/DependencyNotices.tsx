import { useEffect, useRef, useState } from 'react';
import { dependencyNotices } from './bridge';
import s from './PageText.module.css';

export default function DependencyNotices({ close }: { close: () => void }) {
  const dialog = useRef<HTMLDialogElement>(null);
  const [text, setText] = useState<string | null>(null);
  const [error, setError] = useState('');
  useEffect(() => {
    dialog.current?.showModal();
    let disposed = false;
    dependencyNotices().then(value => { if (!disposed) setText(value); }).catch(reason => { if (!disposed) setError(String(reason)); });
    return () => { disposed = true; };
  }, []);
  return <dialog ref={dialog} className={s.dialog} aria-labelledby="notices-title" onCancel={event => { event.preventDefault(); close(); }}>
    <h2 id="notices-title">Third-party notices</h2>
    <p>Dependency licenses and attributions included with this build. Select text and press Ctrl+C to copy.</p>
    {error ? <p role="alert">{error}</p> : text === null ? <p role="status">Loading notices…</p> : <textarea aria-label="Third-party notices" readOnly value={text} spellCheck={false} />}
    <div className={s.actions}><button autoFocus onClick={close}>Close</button></div>
  </dialog>;
}
