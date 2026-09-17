import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import App from './App';
import Viewer from './Viewer';
import { closeDocument, createComment, documentComments, openDocument, saveCopy } from './bridge';
import type { DocumentInfo } from './model';

vi.mock('./bridge', () => ({ native: false, openDocument: vi.fn(), reopenDocument: vi.fn(), closeDocument: vi.fn(), editPages: vi.fn(), saveCopy: vi.fn(), documentComments: vi.fn(), createComment: vi.fn(), updateComment: vi.fn(), deleteComment: vi.fn() }));
vi.mock('./Viewer', () => ({ default: (props: { commentMode: boolean; hand: boolean; onCommentCreate: (page: number, rect: { x: number; y: number; width: number; height: number }) => void }) => <div data-comment-mode={String(props.commentMode)} data-hand={String(props.hand)}><button onClick={() => props.onCommentCreate(0, { x: .1, y: .2, width: .03, height: .03 })}>Place comment</button></div> }));

const document = (revision = 0): DocumentInfo => ({ id: 1, name: 'notes.pdf', path: 'C:/notes.pdf', pages: [{ width: 612, height: 792 }], revision, dirty: revision > 0, can_undo: revision > 0, can_redo: false });
let keydown: ((event: KeyboardEvent) => void) | undefined;
beforeEach(() => {
  vi.clearAllMocks(); const storage = new Map(); keydown = undefined;
  vi.stubGlobal('window', { localStorage: { getItem: (key: string) => storage.get(key) ?? null, setItem: (key: string, value: string) => storage.set(key, value) }, addEventListener: (type: string, listener: (event: KeyboardEvent) => void) => { if (type === 'keydown') keydown = listener; }, removeEventListener: vi.fn() });
});
afterEach(() => vi.unstubAllGlobals());
const button = (ui: ReactTestRenderer, label: string) => ui.root.findByProps({ 'aria-label': label });
async function open(ui: ReactTestRenderer) {
  vi.mocked(openDocument).mockResolvedValue({ status: 'opened', document: document() });
  await act(async () => ui.root.findAllByType('button').find(item => item.children.includes('Open a file'))!.props.onClick());
}

it('creates a note against the current revision and refreshes the active document', async () => {
  vi.mocked(documentComments).mockResolvedValue({ documentId: 1, revision: 0, status: 'supported', reason: null, notes: [] });
  vi.mocked(createComment).mockResolvedValue(document(1));
  let ui!: ReactTestRenderer; act(() => { ui = create(<App />); });
  await open(ui);
  await act(async () => button(ui, 'Add comment').props.onClick());
  expect(ui.root.findByType(Viewer).props.commentMode).toBe(true);
  expect(ui.root.findByType(Viewer).props.hand).toBe(false);
  await act(async () => ui.root.findAllByType('button').find(item => item.children.includes('Place comment'))!.props.onClick());
  act(() => ui.root.findByProps({ 'aria-label': 'Comment text' }).props.onChange({ target: { value: 'Keep this' } }));
  await act(async () => ui.root.findAllByType('button').find(item => item.children.join('') === 'Save comment')!.props.onClick());
  expect(createComment).toHaveBeenCalledWith(1, 0, 0, { x: .1, y: .2, width: .03, height: .03 }, 'Keep this');
  expect(ui.root.findByType(Viewer).props.document).toMatchObject({ id: 1, revision: 1, dirty: true });
  act(() => ui.unmount());
});

it('leaves comment mode through Pan, Select, and the H shortcut without mutating notes', async () => {
  vi.mocked(documentComments).mockResolvedValue({ documentId: 1, revision: 0, status: 'supported', reason: null, notes: [] });
  let ui!: ReactTestRenderer; act(() => { ui = create(<App />); });
  await open(ui);
  await act(async () => button(ui, 'Add comment').props.onClick());
  expect(ui.root.findByType(Viewer).props.commentMode).toBe(true);
  act(() => keydown!({ key: 'h', ctrlKey: false, target: { matches: () => false } } as unknown as KeyboardEvent));
  expect(ui.root.findByType(Viewer).props.commentMode).toBe(false);
  expect(ui.root.findByType(Viewer).props.hand).toBe(true);
  act(() => button(ui, 'Select text on page').props.onClick());
  expect(ui.root.findByType(Viewer).props.commentMode).toBe(false);
  expect(ui.root.findByType(Viewer).props.hand).toBe(false);
  expect(createComment).not.toHaveBeenCalled();
  act(() => ui.unmount());
});

it('leaves panel-launched placement mode with Escape', async () => {
  vi.mocked(documentComments).mockResolvedValue({ documentId: 1, revision: 0, status: 'supported', reason: null, notes: [] });
  let ui!: ReactTestRenderer; act(() => { ui = create(<App />); });
  await open(ui);
  await act(async () => button(ui, 'Comments').props.onClick());
  await act(async () => ui.root.findAllByType('button').find(item => item.children.join('') === 'Add comment on page 1')!.props.onClick());
  expect(ui.root.findByType(Viewer).props.commentMode).toBe(true);
  act(() => keydown!({ key: 'Escape', ctrlKey: false, target: { matches: () => false } } as unknown as KeyboardEvent));
  expect(ui.root.findByType(Viewer).props.commentMode).toBe(false);
  act(() => ui.unmount());
});

it('drops a stale comment query when the active tab changes', async () => {
  let resolveFirst!: (value: { documentId: number; revision: number; status: 'supported'; reason: null; notes: { id: string; page: number; rect: null; contents: string }[] }) => void;
  const first = new Promise<{ documentId: number; revision: number; status: 'supported'; reason: null; notes: { id: string; page: number; rect: null; contents: string }[] }>(resolve => { resolveFirst = resolve; });
  vi.mocked(documentComments).mockImplementation((id: number) => id === 1 ? first : Promise.resolve({ documentId: 2, revision: 0, status: 'supported', reason: null, notes: [{ id: 'fresh', page: 0, rect: null, contents: 'Fresh note' }] }));
  vi.mocked(openDocument).mockResolvedValueOnce({ status: 'opened', document: document() }).mockResolvedValueOnce({ status: 'opened', document: { ...document(), id: 2, name: 'other.pdf', path: 'C:/other.pdf' } });
  let ui!: ReactTestRenderer; act(() => { ui = create(<App />); });
  await act(async () => ui.root.findAllByType('button').find(item => item.children.includes('Open a file'))!.props.onClick());
  await act(async () => button(ui, 'Add comment').props.onClick());
  await act(async () => ui.root.findAllByType('button').find(item => item.children.includes('Open a file'))!.props.onClick());
  await act(async () => { resolveFirst({ documentId: 1, revision: 0, status: 'supported', reason: null, notes: [{ id: 'stale', page: 0, rect: null, contents: 'Stale note' }] }); });
  expect(JSON.stringify(ui.toJSON())).not.toContain('Stale note');
  expect(ui.root.findByType(Viewer).props.commentMode).toBe(false);
  act(() => ui.unmount());
});

it('opens a hidden note from the comments list without creating an on-page anchor', async () => {
  vi.mocked(documentComments).mockResolvedValue({ documentId: 1, revision: 0, status: 'supported', reason: null, notes: [{ id: 'hidden', page: 0, rect: null, contents: 'Hidden note' }] });
  let ui!: ReactTestRenderer; act(() => { ui = create(<App />); });
  await open(ui);
  await act(async () => button(ui, 'Comments').props.onClick());
  const hidden = ui.root.findAllByType('button').find(item => item.findAllByType('span').some(span => span.children.some(child => typeof child === 'string' && child.includes('Hidden note'))))!;
  act(() => hidden.props.onClick());
  expect(ui.root.findByProps({ 'aria-label': 'Comment text' }).props.value).toBe('Hidden note');
  act(() => ui.unmount());
});

it('does not route global shortcuts into the document while a comment editor is open', async () => {
  vi.mocked(documentComments).mockResolvedValue({ documentId: 1, revision: 0, status: 'supported', reason: null, notes: [] });
  let ui!: ReactTestRenderer; act(() => { ui = create(<App />); });
  await open(ui);
  await act(async () => button(ui, 'Add comment').props.onClick());
  await act(async () => ui.root.findAllByType('button').find(item => item.children.includes('Place comment'))!.props.onClick());
  act(() => keydown!({ key: 's', ctrlKey: true, target: { matches: () => false } } as unknown as KeyboardEvent));
  act(() => keydown!({ key: 'h', ctrlKey: false, target: { matches: () => false } } as unknown as KeyboardEvent));
  expect(saveCopy).not.toHaveBeenCalled();
  expect(ui.root.findByProps({ 'aria-label': 'Comment text' })).toBeTruthy();
  expect(ui.root.findByType(Viewer).props.commentMode).toBe(true);
  act(() => ui.unmount());
});

it('keeps the active tab and applies the acknowledged revision when a note save is pending', async () => {
  vi.mocked(documentComments).mockResolvedValue({ documentId: 1, revision: 0, status: 'supported', reason: null, notes: [] });
  let resolveCreate!: (value: DocumentInfo) => void;
  vi.mocked(createComment).mockImplementation(() => new Promise(resolve => { resolveCreate = resolve; }));
  let ui!: ReactTestRenderer; act(() => { ui = create(<App />); });
  await open(ui);
  await act(async () => button(ui, 'Add comment').props.onClick());
  await act(async () => ui.root.findAllByType('button').find(item => item.children.includes('Place comment'))!.props.onClick());
  act(() => ui.root.findByProps({ 'aria-label': 'Comment text' }).props.onChange({ target: { value: 'Persist me' } }));
  act(() => { void ui.root.findAllByType('button').find(item => item.children.join('') === 'Save comment')!.props.onClick(); });
  await act(async () => { await Promise.resolve(); });
  expect(button(ui, 'Close notes.pdf').props.disabled).toBe(true);
  act(() => keydown!({ key: 'Escape', ctrlKey: false, target: { matches: () => false } } as unknown as KeyboardEvent));
  act(() => keydown!({ key: 'w', ctrlKey: true, target: { matches: () => false } } as unknown as KeyboardEvent));
  act(() => { void button(ui, 'Close notes.pdf').props.onClick(); });
  expect(closeDocument).not.toHaveBeenCalled();
  await act(async () => { resolveCreate(document(1)); });
  expect(ui.root.findByType(Viewer).props.document).toMatchObject({ id: 1, revision: 1, dirty: true });
  expect(ui.root.findAllByProps({ 'aria-label': 'Comment text' })).toHaveLength(0);
  act(() => ui.unmount());
});
