import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import App from './App';
import Viewer from './Viewer';
import { editPages, openDocument } from './bridge';
import type { DocumentInfo } from './model';
vi.mock('./bridge', () => ({ native: false, openDocument: vi.fn(), reopenDocument: vi.fn(), closeDocument: vi.fn(), editPages: vi.fn(), saveCopy: vi.fn() }));
vi.mock('./Viewer', () => ({ default: () => <div>Viewer</div> }));
const document = (id: number): DocumentInfo => ({ id, name: `file-${id}.pdf`, path: `C:/file-${id}.pdf`, pages: Array.from({ length: 6 }, () => ({ width: 612, height: 792 })), revision: 0, dirty: false, can_undo: true, can_redo: false });
beforeEach(() => {
  vi.clearAllMocks(); const storage = new Map();
  vi.stubGlobal('window', { localStorage: { getItem: (key: string) => storage.get(key) ?? null, setItem: (key: string, value: string) => storage.set(key, value) }, addEventListener: vi.fn(), removeEventListener: vi.fn() });
});
afterEach(() => vi.unstubAllGlobals());
const tab = (ui: ReactTestRenderer, name: string) => ui.root.findAllByType('button').find(button => button.findAllByType('span').some(span => span.children.includes(name)))!;
async function open(ui: ReactTestRenderer, id: number) {
  vi.mocked(openDocument).mockResolvedValue({ status: 'opened', document: document(id) });
  await act(async () => ui.root.findAllByType('button').find(button => button.children.includes('Open a file'))!.props.onClick());
}
it('restores each tab to its last scrolled or explicitly chosen page', async () => {
  let ui!: ReactTestRenderer; act(() => { ui = create(<App />); });
  await open(ui, 1);
  act(() => ui.root.findByType(Viewer).props.onPage(4));
  await open(ui, 2);
  act(() => ui.root.findByProps({ 'aria-label': 'Page number' }).props.onKeyDown({ key: 'Enter', currentTarget: { value: '3' } }));
  act(() => tab(ui, 'file-1.pdf').props.onClick());
  expect(ui.root.findByType(Viewer).props.target.page).toBe(4);
  act(() => tab(ui, 'file-2.pdf').props.onClick());
  expect(ui.root.findByType(Viewer).props.target.page).toBe(2);
  act(() => ui.unmount());
});
it('clamps the remembered position when page edits shorten a document', async () => {
  let ui!: ReactTestRenderer; act(() => { ui = create(<App />); });
  await open(ui, 1);
  act(() => ui.root.findByType(Viewer).props.onPage(5));
  vi.mocked(editPages).mockResolvedValue({ ...document(1), pages: document(1).pages.slice(0, 2), revision: 1 });
  await act(async () => ui.root.findByProps({ 'aria-label': 'Undo' }).props.onClick());
  await open(ui, 2);
  act(() => tab(ui, 'file-1.pdf').props.onClick());
  expect(ui.root.findByType(Viewer).props.target.page).toBe(1);
  act(() => ui.unmount());
});
