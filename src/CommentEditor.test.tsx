import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { describe, expect, it, vi } from 'vitest';
import CommentEditor, { validateCommentContents } from './CommentEditor';

describe('CommentEditor', () => {
  it('rejects blank, NUL, and oversized comments before calling native work', () => {
    expect(validateCommentContents('  ')).toBe('Enter comment text.');
    expect(validateCommentContents('a\0b')).toBe('Comment text cannot contain NUL characters.');
    expect(validateCommentContents('🙂'.repeat(2049))).toBe('Comment text must be 8 KiB or less.');
  });

  it('saves once, blocks Escape while busy, and exposes delete only for existing notes', async () => {
    const save = vi.fn().mockResolvedValue(undefined), remove = vi.fn().mockResolvedValue(undefined), close = vi.fn();
    let ui!: ReactTestRenderer;
    await act(async () => { ui = create(<CommentEditor draft={{ kind: 'edit', note: { id: 'n1', page: 1, rect: null, contents: 'Before' } }} busy={false} save={save} remove={remove} close={close} />); });
    const textarea = ui.root.findByProps({ 'aria-label': 'Comment text' });
    act(() => textarea.props.onChange({ target: { value: 'After' } }));
    const saveButton = ui.root.findAllByType('button').find(button => button.children.join('') === 'Save comment')!;
    await act(async () => { saveButton.props.onClick(); });
    expect(save).toHaveBeenCalledWith('After');
    expect(close).toHaveBeenCalledTimes(1);
    await act(async () => { ui.update(<CommentEditor draft={{ kind: 'edit', note: { id: 'n1', page: 1, rect: null, contents: 'After' } }} busy save={save} remove={remove} close={close} />); });
    const dialog = ui.root.findByType('dialog');
    act(() => dialog.props.onCancel({ preventDefault: vi.fn() }));
    expect(close).toHaveBeenCalledTimes(1);
    expect(ui.root.findAllByType('button').some(button => button.children.join('') === 'Delete comment')).toBe(true);
  });
});
