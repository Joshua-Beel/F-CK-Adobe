import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { pageText } from './bridge';
import SearchPanel from './SearchPanel';
import type { DocumentInfo } from './model';

vi.mock('./bridge', () => ({ pageText: vi.fn() }));
const document: DocumentInfo = { id: 1, name: 'test.pdf', path: 'test.pdf', pages: [{ width: 612, height: 792 }, { width: 612, height: 792 }], revision: 3, dirty: false, can_undo: false, can_redo: false };
describe('document search', () => {
  beforeEach(() => vi.clearAllMocks());
  async function mount(query = 'needle') {
    let ui!: ReactTestRenderer;
    const go = vi.fn();
    await act(async () => { ui = create(<SearchPanel document={document} go={go} close={vi.fn()} />); });
    act(() => ui.root.findByProps({ 'aria-label': 'Find in document', maxLength: 500 }).props.onChange({ target: { value: query } }));
    return { ui, go };
  }
  const submit = (ui: ReactTestRenderer) => ui.root.findByType('form').props.onSubmit({ preventDefault: vi.fn() });
  it('finds literal case-insensitive text and navigates to the matching current page', async () => {
    vi.mocked(pageText).mockResolvedValueOnce('nothing here').mockResolvedValueOnce('A NEEDLE.* here');
    const { ui, go } = await mount('needle.*');
    await act(async () => { submit(ui); });
    expect(JSON.stringify(ui.toJSON())).toContain('1 matching page.');
    act(() => ui.root.findAllByType('button').find(button => button.children.join('') === 'Go to page 2')!.props.onClick());
    expect(go).toHaveBeenCalledWith(1);
    expect(pageText).toHaveBeenNthCalledWith(2, 1, 1, 3);
    act(() => ui.unmount());
  });
  it('discards pending results when the query changes', async () => {
    let resolve!: (text: string) => void;
    vi.mocked(pageText).mockImplementationOnce(() => new Promise(done => { resolve = done; }));
    const { ui } = await mount();
    act(() => submit(ui));
    act(() => ui.root.findByProps({ maxLength: 500 }).props.onChange({ target: { value: 'different' } }));
    await act(async () => resolve('needle'));
    expect(pageText).toHaveBeenCalledTimes(1);
    expect(JSON.stringify(ui.toJSON())).not.toContain('Go to page');
    act(() => ui.unmount());
  });
  it('reports extraction failure without claiming there are no matches', async () => {
    vi.mocked(pageText).mockRejectedValue(new Error('Document changed. Search again.'));
    const { ui } = await mount();
    await act(async () => submit(ui));
    expect(JSON.stringify(ui.toJSON())).toContain('Search could not finish');
    expect(JSON.stringify(ui.toJSON())).not.toContain('No matching text found');
    act(() => ui.unmount());
  });
  it('supports case-sensitive searches', async () => {
    vi.mocked(pageText).mockResolvedValue('NEEDLE');
    const { ui } = await mount();
    act(() => ui.root.findByProps({ type: 'checkbox' }).props.onChange({ target: { checked: true } }));
    await act(async () => submit(ui));
    expect(JSON.stringify(ui.toJSON())).toContain('No matching text found');
    act(() => ui.unmount());
  });
});
