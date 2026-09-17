import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { check } from '@tauri-apps/plugin-updater';
import Updates from './Updates';

vi.mock('@tauri-apps/api/app', () => ({ getVersion: async () => '0.2.0' }));
vi.mock('@tauri-apps/plugin-updater', () => ({ check: vi.fn() }));
vi.mock('./bridge', () => ({ native: true }));

describe('software updates', () => {
  beforeEach(() => vi.clearAllMocks());
  const mount = async (dirty = false) => {
    const setBusy = vi.fn();
    let ui!: ReactTestRenderer;
    await act(async () => { ui = create(<Updates dirty={dirty} busy={false} setBusy={setBusy} close={vi.fn()} />); });
    return { ui, setBusy };
  };
  it('reports a successful no-update response', async () => {
    vi.mocked(check).mockResolvedValue(null);
    const { ui } = await mount();
    expect(JSON.stringify(ui.toJSON())).toContain('You have the latest released version.');
    act(() => ui.unmount());
  });
  it('blocks installation when any document is dirty, even if called directly', async () => {
    const install = vi.fn(), close = vi.fn();
    vi.mocked(check).mockResolvedValue({ version: '0.3.0', downloadAndInstall: install, close } as never);
    const { ui, setBusy } = await mount(true);
    const button = ui.root.findAllByType('button').find(node => node.children.includes('Install update and restart'))!;
    expect(button.props.disabled).toBe(true);
    await act(async () => { await button.props.onClick(); });
    expect(install).not.toHaveBeenCalled(); expect(setBusy).not.toHaveBeenCalled();
    act(() => ui.unmount()); expect(close).toHaveBeenCalledOnce();
  });
  it('releases the busy guard and shows errors when signature verification fails', async () => {
    const install = vi.fn().mockRejectedValue(new Error('signature verification failed'));
    vi.mocked(check).mockResolvedValue({ version: '0.3.0', downloadAndInstall: install, close: vi.fn() } as never);
    const { ui, setBusy } = await mount();
    await act(async () => { ui.root.findAllByType('button').find(node => node.children.includes('Install update and restart'))!.props.onClick(); });
    expect(setBusy.mock.calls).toEqual([[true], [false]]);
    expect(JSON.stringify(ui.toJSON())).toContain('signature verification failed');
    act(() => ui.unmount());
  });
  it('reports network failure rather than claiming the app is current', async () => {
    vi.mocked(check).mockRejectedValue(new Error('offline'));
    const { ui } = await mount();
    expect(JSON.stringify(ui.toJSON())).toContain('Could not check for updates');
    expect(JSON.stringify(ui.toJSON())).not.toContain('latest released version');
    act(() => ui.unmount());
  });
});
