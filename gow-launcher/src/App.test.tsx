import { act, cleanup, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import App from './App';
import { availableUpdate, createMockApi, setApi, type MockApi } from './lib/backend';
import type { UpdateState } from './lib/types';
import { readBindings } from './lib/keys';

let api: MockApi;

function boot(ready: boolean, update?: UpdateState) {
  api = createMockApi({ ready, update });
  setApi(api);
  return render(<App />);
}

afterEach(cleanup);

describe('main window', () => {
  it('offers to play when the game is ready, and starts it with the saved settings flushed first', async () => {
    boot(true);
    const user = userEvent.setup();
    const patch = vi.spyOn(api, 'patchSettings');
    const start = vi.spyOn(api, 'startGame');
    await user.click(await screen.findByRole('switch', { name: 'Anti-aliasing (FXAA)' }));
    await user.click(await screen.findByRole('button', { name: 'PLAY' }));
    await waitFor(() => expect(start).toHaveBeenCalled());
    // The change was sent before the game started, not left in the debounce window.
    expect(patch).toHaveBeenCalledWith({ fxaa: false });
    expect(patch.mock.invocationCallOrder[0]).toBeLessThan(start.mock.invocationCallOrder[0]);
  });

  it('shows the running state', async () => {
    boot(true);
    await userEvent.setup().click(await screen.findByRole('button', { name: 'PLAY' }));
    expect(await screen.findByRole('button', { name: 'RUNNING' })).toBeDisabled();
    // The mock game closes by itself after 2.5 s: the button comes back and the toast reports the session.
    expect(await screen.findByText('Game closed. Session: 2 min', {}, { timeout: 5000 })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'PLAY' })).toBeEnabled();
  });

  it('keeps the optimization options but not its progress card', async () => {
    boot(true);
    await userEvent.setup().click(await screen.findByRole('button', { name: 'Performance' }));
    // The optimizer is configured and 80 % done in the mock: the player still gets the switches and the button...
    expect(await screen.findByRole('switch', { name: 'Background optimization' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Optimize' })).toBeInTheDocument();
    // ...and no percentage or bar: that is the optimizer's business, not theirs.
    expect(screen.queryByText(/Optimization: \d+ %/)).not.toBeInTheDocument();
    expect(screen.queryByRole('progressbar')).not.toBeInTheDocument();
    expect(screen.queryByText(/compiling in the background/)).not.toBeInTheDocument();
  });

  it('switches sections and the language', async () => {
    boot(true);
    const user = userEvent.setup();
    await user.click(await screen.findByRole('button', { name: 'Other' }));
    expect(screen.getByRole('heading', { name: 'Other' })).toBeInTheDocument();
    await user.click(screen.getByRole('combobox', { name: 'Language' }));
    await user.click(screen.getByRole('option', { name: 'Português (Brasil)' }));
    expect(await screen.findByRole('heading', { name: 'Outros' })).toBeInTheDocument();
  });
});

describe('game failures', () => {
  it('shows the cause of a recognized failure above the log excerpt, and nothing extra for an unknown one', async () => {
    boot(true);
    const user = userEvent.setup();
    await screen.findByRole('button', { name: 'PLAY' });
    act(() => api.emit('game-finished', {
      code: 1, failed: true, seconds: 3, summary: '[gs-hw] GL unavailable: OpenGL 4.6 core context unavailable',
      failureKind: 'failureOpenGl',
    }));
    const dialog = await screen.findByRole('dialog');
    expect(within(dialog).getByText(/Update the graphics driver/)).toBeInTheDocument();
    expect(within(dialog).getByText(/core context unavailable/)).toBeInTheDocument();
    await user.click(within(dialog).getByRole('button', { name: 'OK' }));
    act(() => api.emit('game-finished', { code: 3, failed: true, seconds: 3, summary: 'boom', failureKind: null }));
    const next = await screen.findByRole('dialog');
    expect(within(next).getByText('boom')).toBeInTheDocument();
    expect(within(next).queryByText(/Update the graphics driver/)).not.toBeInTheDocument();
  });
});

describe('rebuild after an update', () => {
  const bootWithRebuild = (rebuildNeeded: boolean) => {
    api = createMockApi({ ready: true, rebuildNeeded });
    setApi(api);
    return render(<App />);
  };

  it('shows nothing when the build is up to date', async () => {
    bootWithRebuild(false);
    await userEvent.setup().click(await screen.findByRole('button', { name: 'Performance' }));
    expect(await screen.findByRole('switch', { name: 'Background optimization' })).toBeInTheDocument();
    expect(screen.queryByText('Rebuild the game to get the latest fixes')).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Rebuild now' })).not.toBeInTheDocument();
  });

  it('announces it once, offers the rebuild in the Performance tab and clears the note when the rebuild succeeds', async () => {
    bootWithRebuild(true);
    const user = userEvent.setup();
    const start = vi.spyOn(api, 'rebuildStart');
    expect(await screen.findByText(/Open Performance to rebuild the game/)).toBeInTheDocument();
    await user.click(await screen.findByRole('button', { name: 'Performance' }));
    expect(await screen.findByText('Rebuild the game to get the latest fixes')).toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: 'Rebuild now' }));
    expect(start).toHaveBeenCalledTimes(1);
    // The optimizer reports the rebuild running and then finished: the need is asked again and is gone.
    const snapshot = { configured: true, compiling: true, captured: 10, compiled: 10, completionRatio: 1, buildProgress: 0.5, status: 'compiling' as const };
    act(() => api.emit('optimizer', snapshot));
    act(() => api.emit('optimizer', { ...snapshot, compiling: false, buildProgress: -1, status: 'idle' }));
    await waitFor(() => expect(screen.queryByText('Rebuild the game to get the latest fixes')).not.toBeInTheDocument());
  });

  it('says so when the rebuild fails, keeps offering it and opens the log', async () => {
    bootWithRebuild(true);
    const user = userEvent.setup();
    const openLog = vi.spyOn(api, 'openOptimizerLog');
    await user.click(await screen.findByRole('button', { name: 'Performance' }));
    expect(await screen.findByRole('button', { name: 'Rebuild now' })).toBeInTheDocument();
    expect(screen.queryByText('The last rebuild of the game failed')).not.toBeInTheDocument();
    const snapshot = { configured: true, compiling: true, captured: 10, compiled: 10, completionRatio: 1, buildProgress: 0.5, status: 'compiling' as const };
    act(() => api.emit('optimizer', snapshot));
    act(() => api.emit('optimizer', { ...snapshot, compiling: false, buildProgress: -1, status: 'error' }));
    expect(await screen.findByText('The last rebuild of the game failed')).toBeInTheDocument();
    expect(screen.getByText(/The rebuild of the game failed\. Open Performance/)).toBeInTheDocument();
    // The failed build keeps its old marker: the offer to rebuild stays.
    expect(screen.getByRole('button', { name: 'Rebuild now' })).toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: 'Open the rebuild log' }));
    expect(openLog).toHaveBeenCalledTimes(1);
  });

  it('disables the button with the reason while the game runs', async () => {
    bootWithRebuild(true);
    const user = userEvent.setup();
    await user.click(await screen.findByRole('button', { name: 'Performance' }));
    expect(await screen.findByRole('button', { name: 'Rebuild now' })).toBeEnabled();
    act(() => api.emit('game-running', true));
    expect(screen.getByRole('button', { name: 'Rebuild now' })).toBeDisabled();
    expect(screen.getByText('Close the game to rebuild.')).toBeInTheDocument();
  });
});

describe('graphics', () => {
  it('turns the widescreen (PCSX2) patch on and off', async () => {
    boot(true);
    const user = userEvent.setup();
    const patch = vi.spyOn(api, 'patchSettings');
    // On by default; off keeps the game's original 4:3 picture (the runtime is not given PC_WIDESCREEN).
    await user.click(await screen.findByRole('switch', { name: 'Widescreen (Hor+)' }));
    await waitFor(() => expect(patch).toHaveBeenCalledWith({ widescreen: false }));
    expect(screen.getByRole('combobox', { name: 'Aspect ratio' })).toBeDisabled();
    // Turning it back on widens the camera at the chosen aspect and moves the window into that aspect.
    await user.click(screen.getByRole('switch', { name: 'Widescreen (Hor+)' }));
    await waitFor(() =>
      expect(patch).toHaveBeenLastCalledWith(expect.objectContaining({ widescreen: true, aspectMode: '16:9', stretch: false })),
    );
    expect(screen.getByRole('combobox', { name: 'Aspect ratio' })).toBeEnabled();
  });
});

describe('controls', () => {
  it('captures a physical key into the second slot and saves the map as JSON', async () => {
    boot(true);
    const user = userEvent.setup();
    const patch = vi.spyOn(api, 'patchSettings');
    await user.click(await screen.findByRole('button', { name: 'Controls' }));
    const row = (await screen.findByText('Special power (cast magic)')).parentElement!;
    const slots = within(row).getAllByRole('button');
    await user.click(slots[1]); // second slot of L2 (the first is Shift)
    expect(await screen.findByRole('heading', { name: /press the new key/i })).toBeInTheDocument();
    window.dispatchEvent(new KeyboardEvent('keydown', { code: 'KeyX', bubbles: true, cancelable: true }));
    await waitFor(() => expect(screen.queryByRole('heading', { name: /press the new key/i })).not.toBeInTheDocument());
    await waitFor(() => expect(patch).toHaveBeenCalled());
    const saved = (patch.mock.calls.at(-1)![0] as { keys: string }).keys;
    expect(readBindings(saved).l2).toEqual([340, 88]);
  });

  it('Escape cancels without changing anything', async () => {
    boot(true);
    const user = userEvent.setup();
    const patch = vi.spyOn(api, 'patchSettings');
    await user.click(await screen.findByRole('button', { name: 'Controls' }));
    const row = (await screen.findByText('Jump')).parentElement!;
    await user.click(within(row).getAllByRole('button')[0]);
    window.dispatchEvent(new KeyboardEvent('keydown', { code: 'Escape', key: 'Escape', bubbles: true, cancelable: true }));
    await waitFor(() => expect(screen.queryByRole('heading', { name: /press the new key/i })).not.toBeInTheDocument());
    expect(patch).not.toHaveBeenCalled();
  });

  it('captures a mouse button', async () => {
    boot(true);
    const user = userEvent.setup();
    const patch = vi.spyOn(api, 'patchSettings');
    await user.click(await screen.findByRole('button', { name: 'Controls' }));
    const row = (await screen.findByText('Jump')).parentElement!;
    await user.click(within(row).getAllByRole('button')[0]);
    window.dispatchEvent(new MouseEvent('mousedown', { button: 1, bubbles: true, cancelable: true }));
    await waitFor(() => expect(patch).toHaveBeenCalled());
    expect(readBindings((patch.mock.calls.at(-1)![0] as { keys: string }).keys).cross).toEqual([1002]);
  });
});

describe('installation wizard', () => {
  beforeEach(() => {
    vi.useRealTimers();
  });

  it('walks from the ISO to a finished installation and adopts the result', async () => {
    boot(false);
    const user = userEvent.setup();
    const install = vi.spyOn(api, 'installStart').mockImplementation(async () => {
      api.emit('install-progress', { stage: 5, stageProgress: 0.5, recentLines: ['[1/2] x'] });
      api.emit('install-done', { ok: true, cancelled: false, error: '', elf: 'C:\\g\\game\\SCUS_973.99', executable: 'C:\\g\\build\\ps2xRuntime\\gow-2005.exe', failureKind: null });
    });
    const complete = vi.spyOn(api, 'installComplete');

    // 0: the ISO is searched on the PC and found.
    expect(await screen.findByText('Choose your God of War ISO')).toBeInTheDocument();
    const next = () => screen.getByRole('button', { name: 'NEXT' });
    await waitFor(() => expect(next()).toBeEnabled(), { timeout: 4000 });
    await user.click(next());
    // 1: location (free space shown, accent-free path accepted).
    expect(await screen.findByText('Where to install')).toBeInTheDocument();
    await waitFor(() => expect(next()).toBeEnabled());
    expect(screen.getByText(/Free space: 412 GB/)).toBeInTheDocument();
    await user.click(next());
    // 2: components, because the mock lacks Build Tools and Git.
    expect(await screen.findByText('Required components')).toBeInTheDocument();
    await user.click(next());
    // 3: summary.
    expect(await screen.findByText('Ready to install')).toBeInTheDocument();
    expect(screen.getByText(/Visual Studio Build Tools/)).toBeInTheDocument();
    await user.click(within(screen.getByRole('dialog')).getByRole('button', { name: 'INSTALL' }));
    // 4 -> 5: success.
    expect(await screen.findByText('All set!')).toBeInTheDocument();
    expect(install).toHaveBeenCalledWith({
      iso: 'D:\\Games\\God of War (USA).iso', destination: 'C:\\GodOfWar-PC', installTools: true, vu1MemoryLimitGb: 4,
    });
    await user.click(screen.getByRole('button', { name: 'OK' }));
    await waitFor(() => expect(complete).toHaveBeenCalledWith({
      destination: 'C:\\GodOfWar-PC', elf: 'C:\\g\\game\\SCUS_973.99', executable: 'C:\\g\\build\\ps2xRuntime\\gow-2005.exe',
    }));
    await waitFor(() => expect(screen.queryByText('All set!')).not.toBeInTheDocument());
  });

  it('blocks the first step when the processor lacks SSE4.1, even with the ISO verified', async () => {
    boot(false);
    vi.spyOn(api, 'wizardInfo').mockResolvedValue({
      environment: { vs: true, cmakeUnusable: false, git: true, installer: true, missing: [], missingPackages: [], installCommand: '', cpuMissing: ['SSE4.1'], kitMissing: [] },
      hasManifest: true, defaultDestination: 'C:\\GodOfWar-PC',
    });
    expect(await screen.findByRole('alert')).toHaveTextContent(/does not support SSE4\.1/);
    // The ISO is found and verified, so only the processor keeps NEXT disabled.
    expect(await screen.findByText(/original ISO verified/, {}, { timeout: 4000 })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'NEXT' })).toBeDisabled();
  });

  it('shows no processor warning when nothing is missing', async () => {
    boot(false);
    const next = () => screen.getByRole('button', { name: 'NEXT' });
    await waitFor(() => expect(next()).toBeEnabled(), { timeout: 4000 });
    expect(screen.queryByText(/does not support SSE4\.1/)).not.toBeInTheDocument();
  });

  it('shows no components when Visual Studio has a usable CMake, and installs nothing', async () => {
    boot(false);
    vi.spyOn(api, 'wizardInfo').mockResolvedValue({
      environment: { vs: true, cmakeUnusable: false, git: true, installer: true, missing: [], missingPackages: [], installCommand: '', cpuMissing: [], kitMissing: [] },
      hasManifest: true, defaultDestination: 'C:\\GodOfWar-PC',
    });
    const user = userEvent.setup();
    const install = vi.spyOn(api, 'installStart').mockResolvedValue(undefined);
    const next = () => screen.getByRole('button', { name: 'NEXT' });
    await waitFor(() => expect(next()).toBeEnabled(), { timeout: 4000 });
    await user.click(next());
    await waitFor(() => expect(next()).toBeEnabled());
    await user.click(next());
    // Location goes straight to the summary: there is no components step.
    expect(await screen.findByText('Ready to install')).toBeInTheDocument();
    expect(screen.queryByText('Required components')).not.toBeInTheDocument();
    await user.click(within(screen.getByRole('dialog')).getByRole('button', { name: 'INSTALL' }));
    await waitFor(() => expect(install).toHaveBeenCalledWith(expect.objectContaining({ installTools: false })));
  });

  it('offers nothing for a Visual Studio without a usable CMake: the installer downloads a portable one', async () => {
    boot(false);
    vi.spyOn(api, 'wizardInfo').mockResolvedValue({
      environment: { vs: true, cmakeUnusable: true, git: true, installer: true, missing: [], missingPackages: [], installCommand: '', cpuMissing: [], kitMissing: [] },
      hasManifest: true, defaultDestination: 'C:\\GodOfWar-PC',
    });
    const user = userEvent.setup();
    const install = vi.spyOn(api, 'installStart').mockResolvedValue(undefined);
    const next = () => screen.getByRole('button', { name: 'NEXT' });
    await waitFor(() => expect(next()).toBeEnabled(), { timeout: 4000 });
    await user.click(next());
    await waitFor(() => expect(next()).toBeEnabled());
    await user.click(next());
    // Location goes straight to the summary, as with a usable CMake: portable CMake and Ninja need no component step.
    expect(await screen.findByText('Ready to install')).toBeInTheDocument();
    expect(screen.queryByText('Required components')).not.toBeInTheDocument();
    await user.click(within(screen.getByRole('dialog')).getByRole('button', { name: 'INSTALL' }));
    await waitFor(() => expect(install).toHaveBeenCalledWith(expect.objectContaining({ installTools: false })));
  });

  it('on Linux lists the missing packages and lets the user continue once a new check finds none', async () => {
    api = createMockApi({ ready: false });
    setApi(api);
    const environment = {
      vs: false, cmakeUnusable: false, git: true, installer: true, missing: ['ninja', 'alsa'], missingPackages: ['ninja-build', 'libasound2-dev'],
      installCommand: 'sudo apt install ninja-build libasound2-dev', cpuMissing: [], kitMissing: [],
    };
    vi.spyOn(api, 'wizardInfo')
      .mockResolvedValueOnce({ environment, hasManifest: true, defaultDestination: '/home/me/gow' })
      .mockResolvedValue({
        environment: { ...environment, vs: true, missing: [], missingPackages: [], installCommand: '', cpuMissing: [], kitMissing: [] },
        hasManifest: true, defaultDestination: '/home/me/gow',
      });
    const user = userEvent.setup();
    render(<App />);
    const next = () => screen.getByRole('button', { name: 'NEXT' });
    await waitFor(() => expect(next()).toBeEnabled(), { timeout: 4000 });
    await user.click(next());
    await waitFor(() => expect(next()).toBeEnabled());
    await user.click(next());
    // 2: nothing to tick; the missing packages and the command to run, and no way forward yet.
    expect(await screen.findByText('Missing: ninja, alsa')).toBeInTheDocument();
    expect(screen.getByText('sudo apt install ninja-build libasound2-dev')).toBeInTheDocument();
    expect(screen.queryByText('Visual Studio Build Tools (C++)')).not.toBeInTheDocument();
    expect(next()).toBeDisabled();
    await user.click(screen.getByRole('button', { name: 'CHECK AGAIN' }));
    await waitFor(() => expect(screen.queryByText('Missing: ninja, alsa')).not.toBeInTheDocument());
    expect(next()).toBeEnabled();
  });

  it('shows the failure and lets the user try again', async () => {
    boot(false);
    const user = userEvent.setup();
    let attempt = 0;
    vi.spyOn(api, 'installStart').mockImplementation(async () => {
      attempt++;
      api.emit('install-done', attempt === 1
        ? { ok: false, cancelled: false, error: 'cmake not found', elf: '', executable: '', failureKind: null }
        : { ok: true, cancelled: false, error: '', elf: 'e', executable: 'x', failureKind: null });
    });
    await waitFor(() => expect(screen.getByRole('button', { name: 'NEXT' })).toBeEnabled(), { timeout: 4000 });
    for (const label of ['NEXT', 'NEXT', 'NEXT']) await user.click(await screen.findByRole('button', { name: label }));
    await user.click(within(await screen.findByRole('dialog')).getByRole('button', { name: 'INSTALL' }));
    expect(await screen.findByText('Could not finish')).toBeInTheDocument();
    expect(screen.getByText('cmake not found')).toBeInTheDocument();
    // An unrecognized failure gets no explanation, only the raw text.
    expect(screen.queryByText(/A download failed/)).not.toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: 'TRY AGAIN' }));
    expect(await screen.findByText('All set!')).toBeInTheDocument();
  });

  it('explains a recognized failure above the raw text', async () => {
    boot(false);
    const user = userEvent.setup();
    vi.spyOn(api, 'installStart').mockImplementation(async () => {
      api.emit('install-done', {
        ok: false, cancelled: false, elf: '', executable: '', failureKind: 'failureDownload',
        error: 'Could not download https://example.org/x.zip (timed out).',
      });
    });
    await waitFor(() => expect(screen.getByRole('button', { name: 'NEXT' })).toBeEnabled(), { timeout: 4000 });
    for (const label of ['NEXT', 'NEXT', 'NEXT']) await user.click(await screen.findByRole('button', { name: label }));
    await user.click(within(await screen.findByRole('dialog')).getByRole('button', { name: 'INSTALL' }));
    expect(await screen.findByText(/A download failed/)).toBeInTheDocument();
    expect(screen.getByText('Could not download https://example.org/x.zip (timed out).')).toBeInTheDocument();
  });

  it('lists the missing kit files and stops at the location step', async () => {
    boot(false);
    vi.spyOn(api, 'wizardInfo').mockResolvedValue({
      environment: {
        vs: true, cmakeUnusable: false, git: true, installer: false, missing: [], missingPackages: [], installCommand: '',
        cpuMissing: [], kitMissing: ['install.ps1', 'bin/ps2-tools.exe'],
      },
      hasManifest: true, defaultDestination: 'C:\GodOfWar-PC',
    });
    const user = userEvent.setup();
    await waitFor(() => expect(screen.getByRole('button', { name: 'NEXT' })).toBeEnabled(), { timeout: 4000 });
    await user.click(screen.getByRole('button', { name: 'NEXT' }));
    expect(await screen.findByText(/Installation kit files are missing: install\.ps1, bin\/ps2-tools\.exe/)).toBeInTheDocument();
    expect(screen.queryByText(/installer script \(install\.ps1/)).not.toBeInTheDocument();
    await screen.findByText(/Free space/);
    expect(screen.getByRole('button', { name: 'NEXT' })).toBeDisabled();
  });

  it('rejects a destination with accents', async () => {
    boot(false);
    const user = userEvent.setup();
    await waitFor(() => expect(screen.getByRole('button', { name: 'NEXT' })).toBeEnabled(), { timeout: 4000 });
    await user.click(screen.getByRole('button', { name: 'NEXT' }));
    const field = await screen.findByDisplayValue('C:\\GodOfWar-PC');
    await user.clear(field);
    await user.type(field, 'C:\\Usuários\\x');
    expect(await screen.findByText(/cannot contain accents/)).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'NEXT' })).toBeDisabled();
  });
});

describe('texture pack installation', () => {
  beforeEach(() => {
    vi.useRealTimers();
  });

  /** The Textures section with custom textures on and the pack card offered; the installation does not start by itself. */
  async function openTheCard() {
    boot(true);
    const install = vi.spyOn(api, 'installTexturePack').mockImplementation(async () => undefined);
    const user = userEvent.setup();
    await user.click(await screen.findByRole('button', { name: 'Textures' }));
    await user.click(await screen.findByRole('switch', { name: 'Custom textures' }));
    await user.click(await screen.findByRole('button', { name: 'Download and install' }));
    await waitFor(() => expect(install).toHaveBeenCalled());
    return user;
  }
  const bar = () => Number(screen.getByRole('progressbar', { name: 'Texture pack' }).getAttribute('aria-valuenow'));
  const progress = (stage: 'downloading' | 'extracting', done: number, total: number, overall: number) =>
    act(() => api.emit('texture-pack-progress', { stage, done, total, overall }));

  it('holds the interface still while it installs and frees it when it ends', async () => {
    await openTheCard();
    // Nothing else can be opened, the game cannot start and the options that change the folder are off.
    expect(screen.getByRole('button', { name: 'Graphics' })).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Performance' })).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Textures' })).toBeEnabled();
    expect(screen.getByRole('button', { name: 'INSTALLING TEXTURES' })).toBeDisabled();
    expect(screen.getByRole('switch', { name: 'Custom textures' })).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Downloading…' })).toBeDisabled();
    expect(screen.getByText(/keep this tab and the launcher open/)).toBeInTheDocument();

    progress('downloading', 2_000_000_000, 4_000_000_000, 0.4);
    expect(screen.getByText(/Downloading: 1.86 of 3.73 GB · 40 %/)).toBeInTheDocument();
    progress('extracting', 120, 420, 0.9);
    expect(screen.getByText('Extracting: 120 / 420 files · 90 %')).toBeInTheDocument();

    act(() => api.emit('texture-pack-done', {
      ok: true, error: null, code: null,
      status: { installed: true, files: 420, bytes: 5_100_000_000, etag: 'e', uploaded: '' },
    }));
    expect(await screen.findByText(/Texture pack installed: 420 files/)).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Graphics' })).toBeEnabled();
    expect(screen.getByRole('button', { name: 'PLAY' })).toBeEnabled();
    expect(screen.queryByRole('progressbar')).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Download and install' })).not.toBeInTheDocument();
  });

  it('never moves the bar backwards, even if a late report arrives', async () => {
    await openTheCard();
    progress('downloading', 3_000_000_000, 4_000_000_000, 0.6);
    expect(bar()).toBe(60);
    progress('downloading', 1_000_000_000, 4_000_000_000, 0.2);
    expect(bar()).toBe(60);
    progress('extracting', 1, 420, 0.81);
    expect(bar()).toBe(81);
  });

  it('frees the interface and says why when the installation fails', async () => {
    await openTheCard();
    progress('downloading', 1_000_000_000, 4_000_000_000, 0.2);
    act(() => api.emit('texture-pack-done', { ok: false, status: null, error: 'could not reach the texture server', code: 'network' }));
    expect(await screen.findByText('The texture pack could not be installed')).toBeInTheDocument();
    expect(screen.getByText('could not reach the texture server')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Graphics' })).toBeEnabled();
    expect(screen.queryByRole('progressbar')).not.toBeInTheDocument();
    // The card is still there, to try again.
    expect(screen.getByRole('button', { name: 'Download and install' })).toBeEnabled();
  });

  it('asks before the window closes during the installation', async () => {
    const user = await openTheCard();
    const forceClose = vi.spyOn(api, 'forceClose');
    progress('downloading', 1_000_000_000, 4_000_000_000, 0.2);

    act(() => api.emit('close-blocked', undefined));
    expect(await screen.findByText('The texture pack is still installing')).toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: 'KEEP INSTALLING' }));
    expect(screen.queryByText('The texture pack is still installing')).not.toBeInTheDocument();
    expect(forceClose).not.toHaveBeenCalled();

    act(() => api.emit('close-blocked', undefined));
    await user.click(await screen.findByRole('button', { name: 'CLOSE ANYWAY' }));
    expect(forceClose).toHaveBeenCalledTimes(1);
  });

  it('starts only one installation, however many times it is asked', async () => {
    const user = await openTheCard();
    const install = vi.mocked(api.installTexturePack);
    expect(install).toHaveBeenCalledTimes(1);
    // A second click on the disabled button does nothing.
    await user.click(screen.getByRole('button', { name: 'Downloading…' }));
    expect(install).toHaveBeenCalledTimes(1);
  });
});

describe('launcher update notice', () => {
  const notice = () => screen.queryByText(/Version 0\.2\.0 is available/);
  const publish = (state: UpdateState) => act(() => api.emit('update-state', state));

  it('shows nothing when there is no newer release', async () => {
    boot(true);
    await screen.findByRole('button', { name: 'PLAY' });
    expect(notice()).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Update' })).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Open release page' })).not.toBeInTheDocument();
  });

  it('shows the version and an Update button that stay put when the section changes', async () => {
    boot(true, availableUpdate());
    const user = userEvent.setup();
    expect(await screen.findByText('Version 0.2.0 is available')).toBeInTheDocument();
    const button = screen.getByRole('button', { name: 'Update' });
    expect(button).toBeEnabled();
    const titleBar = button.closest('header');
    for (const section of ['Performance', 'Controls', 'Other']) {
      await user.click(screen.getByRole('button', { name: section }));
      expect(screen.getByRole('button', { name: 'Update' }).closest('header')).toBe(titleBar);
    }
  });

  it('appears when the check finishes after the window opened', async () => {
    boot(true);
    await screen.findByRole('button', { name: 'PLAY' });
    publish(availableUpdate());
    expect(await screen.findByRole('button', { name: 'Update' })).toBeEnabled();
  });

  it('downloads, shows the progress and then the restart', async () => {
    boot(true, availableUpdate());
    const install = vi.spyOn(api, 'updateInstall');
    await userEvent.setup().click(await screen.findByRole('button', { name: 'Update' }));
    expect(install).toHaveBeenCalledTimes(1);
    publish({ ...availableUpdate(), phase: 'downloading', done: 50, total: 200 });
    expect(screen.getByText('Downloading the update: 25 %')).toBeInTheDocument();
    expect(screen.getByRole('progressbar', { name: 'Update progress' })).toHaveAttribute('aria-valuenow', '25');
    expect(screen.queryByRole('button', { name: 'Update' })).not.toBeInTheDocument();
    publish({ ...availableUpdate(), phase: 'installing', done: 200, total: 200 });
    expect(screen.getByText('Installing the update, the launcher will restart')).toBeInTheDocument();
  });

  it('keeps the bar moving without a known size', async () => {
    boot(true);
    await screen.findByRole('button', { name: 'PLAY' });
    publish({ ...availableUpdate(), phase: 'downloading', done: 1000, total: 0 });
    expect(screen.getByText('Downloading the update')).toBeInTheDocument();
    expect(screen.getByRole('progressbar', { name: 'Update progress' })).not.toHaveAttribute('aria-valuenow');
  });

  it('says what went wrong and retries', async () => {
    boot(true);
    await screen.findByRole('button', { name: 'PLAY' });
    publish({ ...availableUpdate(), phase: 'error', error: 'Download request failed with status: 503' });
    expect(screen.getByText('Could not update to 0.2.0')).toBeInTheDocument();
    expect(screen.getByText('Could not update to 0.2.0').parentElement).toHaveAttribute('title', 'Download request failed with status: 503');
    const install = vi.spyOn(api, 'updateInstall');
    await userEvent.setup().click(screen.getByRole('button', { name: 'Retry' }));
    expect(install).toHaveBeenCalledTimes(1);
    publish({ ...availableUpdate(), phase: 'downloading', done: 1, total: 100 });
    expect(screen.queryByText('Could not update to 0.2.0')).not.toBeInTheDocument();
  });

  it('reports a refusal of the backend instead of failing silently', async () => {
    boot(true, availableUpdate());
    vi.spyOn(api, 'updateInstall').mockRejectedValue('busy:installation');
    await userEvent.setup().click(await screen.findByRole('button', { name: 'Update' }));
    expect(await screen.findByText('Finish the installation first.')).toBeInTheDocument();
  });

  it('disables the button, with the reason, while the game runs', async () => {
    boot(true, availableUpdate());
    const install = vi.spyOn(api, 'updateInstall');
    const user = userEvent.setup();
    await user.click(await screen.findByRole('button', { name: 'PLAY' }));
    await screen.findByRole('button', { name: 'RUNNING' });
    expect(screen.getByRole('button', { name: 'Update' })).toBeDisabled();
    expect(screen.getByText('Close the game to update.')).toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: 'Update' }));
    expect(install).not.toHaveBeenCalled();
    // The mock game closes by itself: the button comes back.
    await waitFor(() => expect(screen.getByRole('button', { name: 'Update' })).toBeEnabled(), { timeout: 5000 });
    expect(screen.queryByText('Close the game to update.')).not.toBeInTheDocument();
  });

  it('disables the button while an installation, an optimization or a texture pack runs', async () => {
    boot(true, availableUpdate());
    await screen.findByRole('button', { name: 'Update' });
    act(() => api.emit('install-progress', { stage: 2, stageProgress: 0.1, recentLines: [] }));
    expect(screen.getByRole('button', { name: 'Update' })).toBeDisabled();
    expect(screen.getByText('Finish the installation first.')).toBeInTheDocument();
    act(() => api.emit('install-done', { ok: true, cancelled: false, error: '', elf: '', executable: '', failureKind: null }));
    expect(screen.getByRole('button', { name: 'Update' })).toBeEnabled();

    act(() => api.emit('optimizer', { configured: true, compiling: true, captured: 1, compiled: 1, completionRatio: 0.5, buildProgress: 0.2, status: 'compiling' }));
    expect(screen.getByRole('button', { name: 'Update' })).toBeDisabled();
    expect(screen.getByText('Wait for the optimization to finish.')).toBeInTheDocument();
    act(() => api.emit('optimizer', { configured: true, compiling: false, captured: 1, compiled: 1, completionRatio: 1, buildProgress: -1, status: 'idle' }));
    expect(screen.getByRole('button', { name: 'Update' })).toBeEnabled();

    act(() => api.emit('texture-pack-progress', { stage: 'downloading', done: 1, total: 100, overall: 0.1 }));
    expect(screen.getByRole('button', { name: 'Update' })).toBeDisabled();
    expect(screen.getByText('Wait for the texture pack to install.')).toBeInTheDocument();
  });

  it('opens the release page, and says why, for a portable copy', async () => {
    boot(true, availableUpdate('portable'));
    const install = vi.spyOn(api, 'updateInstall');
    const open = vi.spyOn(api, 'updateOpenRelease');
    expect(await screen.findByText('Version 0.2.0 is available')).toBeInTheDocument();
    expect(screen.getByText(/This portable copy cannot update itself/)).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Update' })).not.toBeInTheDocument();
    await userEvent.setup().click(screen.getByRole('button', { name: 'Open release page' }));
    expect(open).toHaveBeenCalledTimes(1);
    expect(install).not.toHaveBeenCalled();
  });

  it('opens the release page when the release cannot be installed in place', async () => {
    boot(true, availableUpdate('manual'));
    expect(await screen.findByText('Download it from the release page.')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Open release page' })).toBeEnabled();
  });

  it('is in Portuguese when the language is', async () => {
    boot(true, availableUpdate());
    const user = userEvent.setup();
    await user.click(await screen.findByRole('button', { name: 'Other' }));
    await user.click(screen.getByRole('combobox', { name: 'Language' }));
    await user.click(screen.getByRole('option', { name: 'Português (Brasil)' }));
    expect(await screen.findByText('A versão 0.2.0 está disponível')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Atualizar' })).toBeEnabled();
  });
});

describe('automatic updates setting', () => {
  it('is off by default and saved when turned on', async () => {
    boot(true);
    const patch = vi.spyOn(api, 'patchSettings');
    const user = userEvent.setup();
    await user.click(await screen.findByRole('button', { name: 'Other' }));
    const toggle = screen.getByRole('switch', { name: 'Automatic updates' });
    expect(toggle).not.toBeChecked();
    await user.click(toggle);
    await waitFor(() => expect(patch).toHaveBeenCalledWith({ autoUpdate: true }));
    expect(screen.getByRole('switch', { name: 'Automatic updates' })).toBeChecked();
  });
});
