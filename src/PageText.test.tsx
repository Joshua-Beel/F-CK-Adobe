import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { expect, it, vi } from 'vitest';
import { pageText } from './bridge';
import PageText from './PageText';
import type { DocumentInfo } from './model';
vi.mock('./bridge', () => ({ pageText: vi.fn() }));
const document: DocumentInfo = { id: 1, name: 'test.pdf', path: 'test.pdf', pages: [{ width: 612, height: 792 }], revision: 4, dirty: false, can_undo: false, can_redo: false };
it('offers exact extracted text for selection without interpreting PDF text as HTML', async () => {
  vi.mocked(pageText).mockResolvedValue('<script>plain PDF text</script>\nSecond line');
  const select = vi.fn(), focus = vi.fn(); let ui!: ReactTestRenderer;
  await act(async () => { ui = create(<PageText document={document} page={0} close={vi.fn()} />, { createNodeMock: element => element.type === 'textarea' ? { select, focus } : { showModal: vi.fn() } }); });
  const area = ui.root.findByType('textarea');
  expect(area.props.value).toBe('<script>plain PDF text</script>\nSecond line');
  expect(area.props.readOnly).toBe(true);
  act(() => ui.root.findAllByType('button')[0].props.onClick());
  expect(select).toHaveBeenCalledOnce(); expect(focus).toHaveBeenCalledOnce();
  expect(pageText).toHaveBeenCalledWith(1, 0, 4);
  act(() => ui.unmount());
});
it('distinguishes empty pages from extraction failures', async () => {
  for (const fails of [false, true]) {
    if (fails) vi.mocked(pageText).mockRejectedValue(new Error('Document is closed'));
    else vi.mocked(pageText).mockResolvedValue('');
    let ui!: ReactTestRenderer;
    await act(async () => { ui = create(<PageText document={document} page={0} close={vi.fn()} />); });
    expect(ui.root.findAllByType('textarea')).toHaveLength(0);
    expect(JSON.stringify(ui.toJSON())).toContain(fails ? 'Document is closed' : 'no embedded text');
    expect(ui.root.findAllByType('button')[0].props.disabled).toBe(true);
    act(() => ui.unmount());
  }
});
