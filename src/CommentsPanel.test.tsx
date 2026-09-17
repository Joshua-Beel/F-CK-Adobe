import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { describe, expect, it, vi } from 'vitest';
import CommentsPanel from './CommentsPanel';
import type { DocumentComments } from './bridge';

const document = { id: 2, name: 'commented.pdf', path: 'C:/commented.pdf', pages: [{ width: 612, height: 792 }, { width: 612, height: 792 }], revision: 4, dirty: true, can_undo: true, can_redo: false };
const comments: DocumentComments = { documentId: 2, revision: 4, status: 'supported', reason: null, notes: [{ id: 'visible', page: 0, rect: { x: .1, y: .2, width: .03, height: .03 }, contents: 'Visible' }, { id: 'hidden', page: 1, rect: null, contents: 'Hidden' }] };

describe('CommentsPanel', () => {
  it('lists hidden notes for editing instead of guessing an on-page point', () => {
    const select = vi.fn();
    let ui!: ReactTestRenderer;
    act(() => { ui = create(<CommentsPanel document={document} page={0} comments={comments} error="" selectedId={null} onAdd={vi.fn()} onSelect={select} close={vi.fn()} />); });
    expect(JSON.stringify(ui.toJSON())).toContain('hidden by the current crop');
    const hidden = ui.root.findAllByType('button').at(-1)!;
    act(() => hidden.props.onClick());
    expect(select).toHaveBeenCalledWith(expect.objectContaining({ id: 'hidden', rect: null }));
  });

  it('reports native commenting refusal without offering a create action', () => {
    let ui!: ReactTestRenderer;
    act(() => { ui = create(<CommentsPanel document={document} page={0} comments={{ ...comments, status: 'unsupported', reason: 'Foreign annotations are unsupported.', notes: [] }} error="" selectedId={null} onAdd={vi.fn()} onSelect={vi.fn()} close={vi.fn()} />); });
    expect(JSON.stringify(ui.toJSON())).toContain('Foreign annotations are unsupported.');
    expect(ui.root.findAllByType('button').some(button => button.children.join('').includes('Add comment'))).toBe(false);
  });
});
