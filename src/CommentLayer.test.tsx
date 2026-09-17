import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { describe, expect, it, vi } from 'vitest';
import CommentLayer, { commentRectAtPoint } from './CommentLayer';

describe('CommentLayer', () => {
  it('creates a centered 20-point displayed-page rectangle and clamps it at the edge', () => {
    expect(commentRectAtPoint(.5, .5, 200, 100)).toEqual({ x: .45, y: .4, width: .1, height: .2 });
    expect(commentRectAtPoint(0, 1, 200, 100)).toEqual({ x: 0, y: .8, width: .1, height: .2 });
    expect(commentRectAtPoint(.5, .5, 0, 100)).toBeNull();
  });

  it('uses transparent note hit targets without creating a second icon', () => {
    const select = vi.fn();
    let ui!: ReactTestRenderer;
    act(() => { ui = create(<CommentLayer page={2} pageWidth={612} pageHeight={792} creating={false} onCreate={vi.fn()} onSelect={select} notes={[{ id: 'one', page: 2, rect: { x: .1, y: .2, width: .03, height: .03 }, contents: 'Note' }, { id: 'hidden', page: 2, rect: null, contents: 'Hidden' }]} />); });
    const buttons = ui.root.findAllByType('button');
    expect(buttons).toHaveLength(1);
    expect(buttons[0].props.style).toEqual({ left: '10%', top: '20%', width: '3%', height: '3%' });
    act(() => buttons[0].props.onClick({ stopPropagation: vi.fn() }));
    expect(select).toHaveBeenCalledWith(expect.objectContaining({ id: 'one' }));
  });

  it('creates only in comment mode using the displayed image bounds', () => {
    const createComment = vi.fn();
    let ui!: ReactTestRenderer;
    act(() => { ui = create(<CommentLayer page={0} pageWidth={200} pageHeight={100} creating onCreate={createComment} onSelect={vi.fn()} notes={[]} />); });
    const layer = ui.root.findByProps({ 'data-testid': 'comment-layer' });
    const target = { getBoundingClientRect: () => ({ left: 10, top: 20, width: 400, height: 200 }) };
    act(() => layer.props.onPointerDown({ button: 0, target, currentTarget: target, clientX: 210, clientY: 120 }));
    expect(createComment).toHaveBeenCalledWith(0, { x: .45, y: .4, width: .1, height: .2 });
    act(() => { ui.update(<CommentLayer page={0} pageWidth={200} pageHeight={100} creating={false} onCreate={createComment} onSelect={vi.fn()} notes={[]} />); });
    act(() => layer.props.onPointerDown({ button: 0, target, currentTarget: target, clientX: 210, clientY: 120 }));
    expect(createComment).toHaveBeenCalledTimes(1);
  });
});
