import type { Annotation, DocumentAnnotations } from './bridge';
import type { DocumentInfo } from './model';
import s from './CommentsPanel.module.css';

export default function CommentsPanel({ document, page, annotations, error, selectedId, onAddComment, onAddHighlight, onSelect, close }: { document: DocumentInfo; page: number; annotations: DocumentAnnotations | null; error: string; selectedId: string | null; onAddComment: () => void; onAddHighlight: () => void; onSelect: (annotation: Annotation) => void; close: () => void }) {
  return <aside className={s.panel} aria-label="Comments">
    <div className={s.heading}><h2>Comments</h2><button aria-label="Close comments" onClick={close}>×</button></div>
    {error ? <p role="alert">{error}</p> : !annotations ? <p role="status">Loading comments…</p> : annotations.status === 'unsupported' ? <p role="status">Commenting is unavailable. {annotations.reason || 'This PDF has annotations or structure this build does not change.'}</p> : <>
      <button className={s.add} onClick={onAddComment}>Add comment on page {page + 1}</button><button className={s.add} onClick={onAddHighlight}>Add area highlight on page {page + 1}</button>
      {!annotations.annotations.length ? <p>No comments or highlights yet.</p> : <div className={s.notes}>{annotations.annotations.map(annotation => <button key={annotation.id} className={annotation.id === selectedId ? s.selected : ''} onClick={() => onSelect(annotation)}><strong>{annotation.kind === 'note' ? 'Comment' : 'Highlight'} · Page {annotation.page + 1}</strong><span>{annotation.rect ? annotation.contents || 'No description' : `${annotation.contents || 'No description'} (hidden by the current crop)`}</span></button>)}</div>}
    </>}
  </aside>;
}
