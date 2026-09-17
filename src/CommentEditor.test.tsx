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
    await act(async () => { ui = create(<CommentEditor draft={{ kind: 'edit', annotation: { id: 'n1', kind: 'note', page: 1, rect: null, contents: 'Before' } }} busy={false} save={save} remove={remove} close={close} />); });
    const textarea = ui.root.findByProps({ 'aria-label': 'Comment text' });
    act(() => textarea.props.onChange({ target: { value: 'After' } }));
    const saveButton = ui.root.findAllByType('button').find(button => button.children.join('') === 'Save comment')!;
    await act(async () => { saveButton.props.onClick(); });
    expect(save).toHaveBeenCalledWith('After');
    expect(close).toHaveBeenCalledTimes(1);
    await act(async () => { ui.update(<CommentEditor draft={{ kind: 'edit', annotation: { id: 'n1', kind: 'note', page: 1, rect: null, contents: 'After' } }} busy save={save} remove={remove} close={close} />); });
    const dialog = ui.root.findByType('dialog');
    act(() => dialog.props.onCancel({ preventDefault: vi.fn() }));
    expect(close).toHaveBeenCalledTimes(1);
    expect(ui.root.findAllByType('button').some(button => button.children.join('') === 'Delete comment')).toBe(true);
  });

  it('allows an empty area-highlight description and preserves nonblank Unicode text', async () => {
    const save = vi.fn().mockResolvedValue(undefined), close = vi.fn();
    let ui!: ReactTestRenderer;
    await act(async () => { ui = create(<CommentEditor draft={{ kind: 'create', type: 'area-highlight', page: 0, rect: { x: .1, y: .2, width: .3, height: .4 } }} busy={false} save={save} close={close} />); });
    expect(ui.root.findByProps({ 'aria-label': 'Highlight description' }).props.value).toBe('');
    await act(async () => ui.root.findAllByType('button').find(button => button.children.join('') === 'Save area highlight')!.props.onClick());
    expect(save).toHaveBeenCalledWith('');
    await act(async () => { ui = create(<CommentEditor draft={{ kind: 'create', type: 'area-highlight', page: 0, rect: { x: .1, y: .2, width: .3, height: .4 } }} busy={false} save={save} close={close} />); });
    act(() => ui.root.findByProps({ 'aria-label': 'Highlight description' }).props.onChange({ target: { value: '  🙂  ' } }));
    await act(async () => ui.root.findAllByType('button').find(button => button.children.join('') === 'Save area highlight')!.props.onClick());
    expect(save).toHaveBeenLastCalledWith('  🙂  ');
  });

  it('does not erase a nonblank Unicode whitespace body while native normalizes optional text', async () => {
    const save = vi.fn().mockResolvedValue(undefined), close = vi.fn();
    let ui!: ReactTestRenderer;
    await act(async () => { ui = create(<CommentEditor draft={{ kind: 'edit', annotation: { id: 'h1', kind: 'highlight', page: 0, rect: null, contents: '\uFEFF' } }} busy={false} save={save} close={close} />); });
    await act(async () => ui.root.findAllByType('button').find(button => button.children.join('') === 'Save highlight')!.props.onClick());
    expect(save).toHaveBeenCalledWith('\uFEFF');
  });

  it('uses the same optional body editor for a selected-text highlight', async () => {
    const save = vi.fn().mockResolvedValue(undefined), close = vi.fn();
    let ui!: ReactTestRenderer;
    await act(async () => { ui = create(<CommentEditor draft={{ kind: 'create-text-highlight', page: 2, start: 4, end: 9 }} busy={false} save={save} close={close} />); });
    expect(ui.root.findByType('h2').children.join('')).toBe('New text highlight on page 3');
    await act(async () => ui.root.findAllByType('button').find(button => button.children.join('') === 'Save text highlight')!.props.onClick());
    expect(save).toHaveBeenCalledWith('');
  });
});
