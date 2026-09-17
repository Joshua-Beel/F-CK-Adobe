import type { CommentNote, DocumentComments } from './bridge';
import type { DocumentInfo } from './model';
import s from './CommentsPanel.module.css';

export default function CommentsPanel({ document, page, comments, error, selectedId, onAdd, onSelect, close }: { document: DocumentInfo; page: number; comments: DocumentComments | null; error: string; selectedId: string | null; onAdd: () => void; onSelect: (note: CommentNote) => void; close: () => void }) {
  return <aside className={s.panel} aria-label="Comments">
    <div className={s.heading}><h2>Comments</h2><button aria-label="Close comments" onClick={close}>×</button></div>
    {error ? <p role="alert">{error}</p> : !comments ? <p role="status">Loading comments…</p> : comments.status === 'unsupported' ? <p role="status">Commenting is unavailable. {comments.reason || 'This PDF has annotations or structure this build does not change.'}</p> : <>
      <button className={s.add} onClick={onAdd}>Add comment on page {page + 1}</button>
      {!comments.notes.length ? <p>No comments yet.</p> : <div className={s.notes}>{comments.notes.map(note => <button key={note.id} className={note.id === selectedId ? s.selected : ''} onClick={() => onSelect(note)}><strong>Page {note.page + 1}</strong><span>{note.rect ? note.contents : `${note.contents} (hidden by the current crop)`}</span></button>)}</div>}
    </>}
  </aside>;
}
