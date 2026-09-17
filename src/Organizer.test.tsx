import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import Organizer from './Organizer';
import type { DocumentInfo } from './model';

vi.mock('./bridge', () => ({ renderPage: vi.fn() }));
beforeEach(() => vi.stubGlobal('IntersectionObserver', class { observe() {} disconnect() {} }));
afterEach(() => vi.unstubAllGlobals());
const document: DocumentInfo = { id: 1, name: 'test.pdf', path: 'test.pdf', pages: Array.from({ length: 6 }, () => ({ width: 612, height: 792 })), revision: 0, dirty: false, can_undo: false, can_redo: false };
const pageButton = (ui: ReactTestRenderer, page: number) => ui.root.findByProps({ 'aria-label': `Select page ${page}` });
const input = (ui: ReactTestRenderer) => ui.root.findByProps({ 'aria-label': 'Destination page position' });

it('moves to the requested final position and keeps the moved page selected', async () => {
  const edit = vi.fn().mockResolvedValue(true); let ui!: ReactTestRenderer;
  act(() => { ui = create(<Organizer document={document} busy={false} edit={edit} save={vi.fn()} close={vi.fn()} />); });
  act(() => input(ui).props.onChange({ target: { value: '6' } }));
  await act(async () => input(ui).props.onKeyDown({ key: 'Enter', preventDefault: vi.fn() }));
  expect(edit).toHaveBeenCalledWith({ kind: 'move', from: 0, to: 5 });
  expect(pageButton(ui, 6).props['aria-pressed']).toBe(true);
  expect(pageButton(ui, 1).props['aria-pressed']).toBe(false);
  expect(ui.root.findByProps({ 'aria-label': 'Page selection range' }).props.value).toBe('6');
  act(() => ui.unmount());
});

it('rejects malformed destinations and preserves selection when the native move fails', async () => {
  const edit = vi.fn().mockResolvedValue(false); let ui!: ReactTestRenderer;
  act(() => { ui = create(<Organizer document={document} busy={false} edit={edit} save={vi.fn()} close={vi.fn()} />); });
  for (const value of ['', '0', '7', '-1', '2.5', '1e0', 'Infinity', '99999999999999999999']) {
    act(() => input(ui).props.onChange({ target: { value } }));
    await act(async () => input(ui).props.onKeyDown({ key: 'Enter', preventDefault: vi.fn() }));
    expect(ui.root.findByProps({ role: 'alert' }).children.join('')).toContain('between 1 and 6');
  }
  expect(edit).not.toHaveBeenCalled();
  act(() => input(ui).props.onChange({ target: { value: '4' } }));
  await act(async () => input(ui).props.onKeyDown({ key: 'Enter', preventDefault: vi.fn() }));
  expect(edit).toHaveBeenCalledOnce();
  expect(pageButton(ui, 1).props['aria-pressed']).toBe(true);
  act(() => ui.unmount());
});

it('bounds shift selection after deleting pages at the end', () => {
  let ui!: ReactTestRenderer;
  const props = { busy: false, edit: vi.fn(), save: vi.fn(), close: vi.fn() };
  act(() => { ui = create(<Organizer {...props} document={document} />); });
  act(() => pageButton(ui, 6).props.onClick({ shiftKey: false, ctrlKey: false }));
  act(() => ui.update(<Organizer {...props} document={{ ...document, pages: document.pages.slice(0, 2), revision: 1 }} />));
  act(() => pageButton(ui, 1).props.onClick({ shiftKey: true, ctrlKey: false }));
  act(() => ui.root.findAllByType('button').find(button => button.children.includes(' Extract'))!.props.onClick());
  expect(props.save).toHaveBeenCalledWith([0, 1]);
  expect(input(ui).props.disabled).toBe(true);
  act(() => ui.unmount());
});
