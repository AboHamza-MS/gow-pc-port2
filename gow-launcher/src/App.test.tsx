import { act, cleanup, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import App from './App';
import { createMockApi, setApi, type MockApi } from './lib/backend';
import { readBindings } from './lib/keys';

let api: MockApi;

function boot(ready: boolean) {
  api = createMockApi({ ready });
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
      api.emit('install-done', { ok: true, cancelled: false, error: '', elf: 'C:\\g\\game\\SCUS_973.99', executable: 'C:\\g\\build\\ps2xRuntime\\gow-2005.exe' });
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

  it('shows no components when Visual Studio has a usable CMake, and installs nothing', async () => {
    boot(false);
    vi.spyOn(api, 'wizardInfo').mockResolvedValue({
      environment: { vs: true, cmakeUnusable: false, git: true, installer: true, missing: [], missingPackages: [], installCommand: '' },
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

  it('offers Visual Studio 2022 next to a Visual Studio without a usable CMake and installs it only when ticked', async () => {
    boot(false);
    vi.spyOn(api, 'wizardInfo').mockResolvedValue({
      environment: { vs: true, cmakeUnusable: true, git: true, installer: true, missing: [], missingPackages: [], installCommand: '' },
      hasManifest: true, defaultDestination: 'C:\\GodOfWar-PC',
    });
    const user = userEvent.setup();
    const install = vi.spyOn(api, 'installStart').mockResolvedValue(undefined);
    const next = () => screen.getByRole('button', { name: 'NEXT' });
    await waitFor(() => expect(next()).toBeEnabled(), { timeout: 4000 });
    await user.click(next());
    await waitFor(() => expect(next()).toBeEnabled());
    await user.click(next());
    // 2: it counts as missing, with its own explanation and the box ticked by default.
    expect(await screen.findByText('Required components')).toBeInTheDocument();
    expect(screen.getByText(/no CMake that can build the game/)).toBeInTheDocument();
    await user.click(next());
    expect(await screen.findByText('Ready to install')).toBeInTheDocument();
    await user.click(within(screen.getByRole('dialog')).getByRole('button', { name: 'INSTALL' }));
    await waitFor(() => expect(install).toHaveBeenCalledWith(expect.objectContaining({ installTools: true })));
  });

  it('on Linux lists the missing packages and lets the user continue once a new check finds none', async () => {
    api = createMockApi({ ready: false });
    setApi(api);
    const environment = {
      vs: false, cmakeUnusable: false, git: true, installer: true, missing: ['ninja', 'alsa'], missingPackages: ['ninja-build', 'libasound2-dev'],
      installCommand: 'sudo apt install ninja-build libasound2-dev',
    };
    vi.spyOn(api, 'wizardInfo')
      .mockResolvedValueOnce({ environment, hasManifest: true, defaultDestination: '/home/me/gow' })
      .mockResolvedValue({
        environment: { ...environment, vs: true, missing: [], missingPackages: [], installCommand: '' },
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
        ? { ok: false, cancelled: false, error: 'cmake not found', elf: '', executable: '' }
        : { ok: true, cancelled: false, error: '', elf: 'e', executable: 'x' });
    });
    await waitFor(() => expect(screen.getByRole('button', { name: 'NEXT' })).toBeEnabled(), { timeout: 4000 });
    for (const label of ['NEXT', 'NEXT', 'NEXT']) await user.click(await screen.findByRole('button', { name: label }));
    await user.click(within(await screen.findByRole('dialog')).getByRole('button', { name: 'INSTALL' }));
    expect(await screen.findByText('Could not finish')).toBeInTheDocument();
    expect(screen.getByText('cmake not found')).toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: 'TRY AGAIN' }));
    expect(await screen.findByText('All set!')).toBeInTheDocument();
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
