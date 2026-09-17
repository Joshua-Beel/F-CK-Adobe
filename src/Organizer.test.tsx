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
  act(() => { ui = create(<Organizer document={document} busy={false} edit={edit} save={vi.fn()} split={vi.fn()} crop={vi.fn()} close={vi.fn()} />); });
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
  act(() => { ui = create(<Organizer document={document} busy={false} edit={edit} save={vi.fn()} split={vi.fn()} crop={vi.fn()} close={vi.fn()} />); });
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
  const props = { busy: false, edit: vi.fn(), save: vi.fn(), split: vi.fn(), crop: vi.fn(), close: vi.fn() };
  act(() => { ui = create(<Organizer {...props} document={document} />); });
  act(() => pageButton(ui, 6).props.onClick({ shiftKey: false, ctrlKey: false }));
  act(() => ui.update(<Organizer {...props} document={{ ...document, pages: document.pages.slice(0, 2), revision: 1 }} />));
  act(() => pageButton(ui, 1).props.onClick({ shiftKey: true, ctrlKey: false }));
  act(() => ui.root.findAllByType('button').find(button => button.children.includes(' Extract'))!.props.onClick());
  expect(props.save).toHaveBeenCalledWith([0, 1]);
  expect(input(ui).props.disabled).toBe(true);
  act(() => ui.unmount());
});

it('splits the current page plan without editing or saving the working document', async () => {
  const edit = vi.fn(), save = vi.fn(), split = vi.fn().mockResolvedValue({ folder: 'C:/splits', files: [{ path: 'C:/splits/part-1.pdf', first_page: 1, last_page: 6, page_count: 6 }] });
  let ui!: ReactTestRenderer;
  act(() => { ui = create(<Organizer document={{ ...document, dirty: true, revision: 3 }} busy={false} edit={edit} save={save} split={split} crop={vi.fn()} close={vi.fn()} />); });
  act(() => ui.root.findAllByType('button').find(button => button.children.includes(' Split'))!.props.onClick());
  await act(async () => ui.root.findAllByType('button').find(button => button.children.includes('Split PDF'))!.props.onClick());
  expect(split).toHaveBeenCalledWith(1);
  expect(edit).not.toHaveBeenCalled(); expect(save).not.toHaveBeenCalled();
  expect(JSON.stringify(ui.toJSON())).toContain('Split complete');
  act(() => ui.unmount());
});

it('crops exactly one selected current page through the crop dialog', async () => {
  const crop = vi.fn().mockResolvedValue(undefined); let ui!: ReactTestRenderer;
  act(() => { ui = create(<Organizer document={document} busy={false} edit={vi.fn()} save={vi.fn()} split={vi.fn()} crop={crop} close={vi.fn()} />); });
  act(() => pageButton(ui, 3).props.onClick({ shiftKey: false, ctrlKey: false, metaKey: false }));
  act(() => ui.root.findAllByType('button').find(button => button.children.includes(' Crop'))!.props.onClick());
  expect(ui.root.findByProps({ id: 'crop-title' }).children.join('')).toBe('Crop page 3');
  for (const edge of ['Top', 'Right', 'Bottom', 'Left']) act(() => ui.root.findByProps({ 'aria-label': `${edge} crop inset` }).props.onChange({ target: { value: '12' } }));
  await act(async () => ui.root.findAllByType('button').find(button => button.children.join('') === 'Apply crop')!.props.onClick());
  expect(crop).toHaveBeenCalledWith(2, { x: 12 / 612, y: 12 / 792, width: 588 / 612, height: 768 / 792 });
  act(() => ui.unmount());
});

it('starts on the current page, preserves later user selection, and clamps it when pages disappear', async () => {
  const crop = vi.fn().mockResolvedValue(undefined);
  const pages = document.pages.map((page, index) => index === 5 ? { width: 792, height: 612 } : page);
  const props = { busy: false, edit: vi.fn(), save: vi.fn(), split: vi.fn(), crop, close: vi.fn() };
  let ui!: ReactTestRenderer;
  act(() => { ui = create(<Organizer {...props} document={{ ...document, pages }} currentPage={5} />); });
  expect(ui.root.findByProps({ 'aria-label': 'Page selection range' }).props.value).toBe('6');
  expect(pageButton(ui, 6).props['aria-pressed']).toBe(true);
  act(() => ui.root.findAllByType('button').find(button => button.children.includes(' Crop'))!.props.onClick());
  expect(ui.root.findByProps({ id: 'crop-title' }).children.join('')).toBe('Crop page 6');
  for (const [edge, value] of [['Top', '72'], ['Right', '36'], ['Bottom', '36'], ['Left', '72']]) act(() => ui.root.findByProps({ 'aria-label': `${edge} crop inset` }).props.onChange({ target: { value } }));
  await act(async () => ui.root.findAllByType('button').find(button => button.children.join('') === 'Apply crop')!.props.onClick());
  expect(crop).toHaveBeenCalledWith(5, { x: 72 / 792, y: 72 / 612, width: 684 / 792, height: 504 / 612 });
  act(() => pageButton(ui, 2).props.onClick({ shiftKey: false, ctrlKey: false, metaKey: false }));
  act(() => ui.update(<Organizer {...props} document={{ ...document, pages }} currentPage={0} />));
  expect(pageButton(ui, 2).props['aria-pressed']).toBe(true);
  act(() => ui.update(<Organizer {...props} document={{ ...document, pages: pages.slice(0, 1), revision: 1 }} currentPage={0} />));
  expect(ui.root.findByProps({ 'aria-label': 'Page selection range' }).props.value).toBe('1');
  expect(pageButton(ui, 1).props['aria-pressed']).toBe(true);
  act(() => ui.unmount());
});

it('keeps an intentionally empty selection after Ctrl+click or a successful delete', async () => {
  const props = { busy: false, edit: vi.fn().mockResolvedValue(true), save: vi.fn(), split: vi.fn(), crop: vi.fn(), close: vi.fn() };
  let ui!: ReactTestRenderer;
  act(() => { ui = create(<Organizer {...props} document={document} currentPage={2} />); });
  expect(pageButton(ui, 3).props['aria-pressed']).toBe(true);
  act(() => pageButton(ui, 3).props.onClick({ shiftKey: false, ctrlKey: true, metaKey: false }));
  expect(pageButton(ui, 3).props['aria-pressed']).toBe(false);
  expect(ui.root.findAllByType('button').find(button => button.children.includes(' Crop'))!.props.disabled).toBe(true);
  act(() => pageButton(ui, 3).props.onClick({ shiftKey: false, ctrlKey: false, metaKey: false }));
  act(() => ui.root.findAllByType('button').find(button => button.children.includes(' Delete'))!.props.onClick());
  await act(async () => ui.root.findAllByType('button').find(button => button.children.join('') === 'Delete pages')!.props.onClick());
  expect(props.edit).toHaveBeenCalledWith({ kind: 'delete', pages: [2] });
  expect(pageButton(ui, 3).props['aria-pressed']).toBe(false);
  expect(ui.root.findAllByType('button').find(button => button.children.includes(' Crop'))!.props.disabled).toBe(true);
  act(() => ui.unmount());
});
