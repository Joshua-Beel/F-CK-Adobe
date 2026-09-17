import type { PointerEvent } from 'react';
import type { CommentNote, CommentRect } from './bridge';
import s from './CommentLayer.module.css';

const clamp = (value: number, lower: number, upper: number) => Math.max(lower, Math.min(upper, value));
const positioned = (rect: CommentRect) => ({ left: `${rect.x * 100}%`, top: `${rect.y * 100}%`, width: `${rect.width * 100}%`, height: `${rect.height * 100}%` });

export function commentRectAtPoint(x: number, y: number, pageWidth: number, pageHeight: number): CommentRect | null {
  if (![x, y, pageWidth, pageHeight].every(Number.isFinite) || pageWidth < 1 || pageHeight < 1) return null;
  const width = Math.min(20, pageWidth) / pageWidth;
  const height = Math.min(20, pageHeight) / pageHeight;
  return { x: clamp(x - width / 2, 0, 1 - width), y: clamp(y - height / 2, 0, 1 - height), width, height };
}

export default function CommentLayer({ page, pageWidth, pageHeight, notes, creating, onCreate, onSelect }: { page: number; pageWidth: number; pageHeight: number; notes: CommentNote[]; creating: boolean; onCreate: (page: number, rect: CommentRect) => void; onSelect: (note: CommentNote) => void }) {
  const create = (event: PointerEvent<HTMLDivElement>) => {
    if (!creating || event.button !== 0 || event.target !== event.currentTarget) return;
    const bounds = event.currentTarget.getBoundingClientRect();
    const rect = commentRectAtPoint((event.clientX - bounds.left) / bounds.width, (event.clientY - bounds.top) / bounds.height, pageWidth, pageHeight);
    if (rect) onCreate(page, rect);
  };
  return <div className={`${s.layer} ${creating ? s.creating : ''}`} data-testid="comment-layer" onPointerDown={create}>
    {notes.filter(note => note.rect).map(note => <button key={note.id} type="button" className={s.note} aria-label={`Edit comment on page ${page + 1}`} title="Edit comment" style={positioned(note.rect!)} onPointerDown={event => event.stopPropagation()} onClick={event => { event.stopPropagation(); onSelect(note); }} />)}
  </div>;
}
