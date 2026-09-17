import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { describe, expect, it, vi } from 'vitest';
import FillFormsDialog, { formPatches, validateFormPatches } from './FillFormsDialog';
import type { DocumentFormFields, SavedCopy } from './bridge';
import type { DocumentInfo } from './model';

const document: DocumentInfo = { id: 4, name: 'form.pdf', path: 'C:/form.pdf', pages: [{ width: 612, height: 792 }], revision: 7, dirty: true, can_undo: true, can_redo: false };
const fields: DocumentFormFields = { documentId: 4, revision: 7, status: 'supported', reason: null, input: 'printable-ascii', valueByteLimit: 4096, fields: [{ fieldId: 'name', name: 'Name', page: 0, value: 'Ada', maxLength: 12 }, { fieldId: 'city', name: 'City', page: 0, value: '', maxLength: null }] };
const output: SavedCopy = { path: 'C:/filled.pdf', document: { ...document, id: 8, name: 'filled.pdf', path: 'C:/filled.pdf', revision: 0, dirty: false, can_undo: false } };
const save = (ui: ReactTestRenderer) => ui.root.findAllByType('button').find(button => button.children.join('') === 'Save filled copy')!;

async function mount({ formFields = fields, error = '', busy = false, fill = vi.fn().mockResolvedValue(output), close = vi.fn() }: { formFields?: DocumentFormFields | null; error?: string; busy?: boolean; fill?: ReturnType<typeof vi.fn>; close?: ReturnType<typeof vi.fn> } = {}) {
  let ui!: ReactTestRenderer;
  await act(async () => { ui = create(<FillFormsDialog document={document} formFields={formFields} error={error} busy={busy} fill={fill} close={close} />, { createNodeMock: element => element.type === 'dialog' ? { showModal: vi.fn() } : null }); });
  return { ui, fill, close };
}

describe('FillFormsDialog', () => {
  it('uses changed printable-ASCII patches only and states new-copy source preservation', async () => {
    const { ui, fill, close } = await mount();
    expect(JSON.stringify(ui.toJSON())).toContain('Your open source PDF stays unchanged.');
    act(() => ui.root.findByProps({ 'aria-label': 'Name' }).props.onChange({ target: { value: 'Ada Lovelace' } }));
    await act(async () => save(ui).props.onClick());
    expect(fill).toHaveBeenCalledWith(4, 7, [{ fieldId: 'name', value: 'Ada Lovelace' }]);
    expect(close).toHaveBeenCalledOnce();
    act(() => ui.unmount());
  });

  it('validates printable ASCII, UTF-8 byte limits, and field character limits before native work', () => {
    const values = { name: 'Ada', city: 'Paris' };
    expect(formPatches(fields.fields, values)).toEqual([{ fieldId: 'city', value: 'Paris' }]);
    expect(validateFormPatches(fields.fields, { name: 'Ada', city: '' }, 4096)).toContain('Change at least one');
    expect(validateFormPatches(fields.fields, { name: 'Åda', city: '' }, 4096)).toContain('printable ASCII');
    expect(validateFormPatches([{ ...fields.fields[0], maxLength: 3 }], { name: 'ABCD' }, 4096)).toContain('3-character');
    expect(validateFormPatches([{ ...fields.fields[0], maxLength: null }], { name: 'ABCD' }, 3)).toContain('3-byte');
  });

  it('shows unsupported native reasons without partial fields', async () => {
    const { ui } = await mount({ formFields: { ...fields, status: 'unsupported', reason: 'Unsupported field appearance.', fields: [] } });
    expect(ui.root.findByProps({ role: 'status' }).children.join('')).toContain('Unsupported field appearance.');
    expect(ui.root.findAllByProps({ 'aria-label': 'Name' })).toHaveLength(0);
    expect(save(ui).props.disabled).toBe(true);
    act(() => ui.unmount());
  });

  it('keeps drafts after native cancellation or failure and blocks Escape and duplicate submissions while busy', async () => {
    const canceled = await mount({ fill: vi.fn().mockResolvedValue(null) });
    act(() => canceled.ui.root.findByProps({ 'aria-label': 'City' }).props.onChange({ target: { value: 'Paris' } }));
    await act(async () => save(canceled.ui).props.onClick());
    expect(canceled.close).not.toHaveBeenCalled();
    expect(canceled.ui.root.findByProps({ 'aria-label': 'City' }).props.value).toBe('Paris');
    expect(canceled.ui.root.findByProps({ role: 'status' }).children.join('')).toContain('Saving was canceled');
    act(() => canceled.ui.unmount());

    let reject!: (reason: Error) => void;
    const fill = vi.fn().mockImplementation(() => new Promise<SavedCopy | null>((_, fail) => { reject = fail; }));
    const pending = await mount({ fill });
    act(() => pending.ui.root.findByProps({ 'aria-label': 'City' }).props.onChange({ target: { value: 'Paris' } }));
    act(() => { save(pending.ui).props.onClick(); save(pending.ui).props.onClick(); });
    expect(fill).toHaveBeenCalledOnce();
    act(() => pending.ui.root.findByType('dialog').props.onCancel({ preventDefault: vi.fn() }));
    expect(pending.close).not.toHaveBeenCalled();
    await act(async () => reject(new Error('Document changed.')));
    expect(pending.ui.root.findByProps({ role: 'alert' }).children.join('')).toContain('Document changed');
    expect(pending.ui.root.findByProps({ 'aria-label': 'City' }).props.value).toBe('Paris');
    act(() => pending.ui.unmount());
  });
});
