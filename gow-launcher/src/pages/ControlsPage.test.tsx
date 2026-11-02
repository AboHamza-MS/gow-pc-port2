import { cleanup, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import App from '../App';
import { createMockApi, dongleDevices, setApi, type MockApi } from '../lib/backend';
import type { LiveDevice, PadDeviceList } from '../lib/types';
import { serializePadMap, presetLayout } from '../lib/padmap';

let api: MockApi;

afterEach(cleanup);

async function openControls(padDevices?: PadDeviceList | null) {
  api = createMockApi({ ready: true, padDevices });
  setApi(api);
  render(<App />);
  const user = userEvent.setup();
  await user.click(await screen.findByRole('button', { name: 'Controls' }));
  const patch = vi.spyOn(api, 'patchSettings');
  return { user, patch };
}

const choose = async (user: ReturnType<typeof userEvent.setup>, combo: string, option: string) => {
  await user.click(screen.getByRole('combobox', { name: combo }));
  await user.click(screen.getByRole('option', { name: option }));
};

describe('custom gamepad button map', () => {
  it('starts from the layout in use and shows one row per physical button', async () => {
    const { user, patch } = await openControls();
    expect(screen.queryByRole('combobox', { name: 'Start presses' })).not.toBeInTheDocument();
    await choose(user, 'Gamepad layout', 'Custom (every button)');
    // The first edit seeds the map from the classic layout the player had, so nothing else changes.
    const seeded = serializePadMap(presetLayout(false, ['cross', 'circle', 'square', 'triangle']));
    await waitFor(() => expect(patch).toHaveBeenCalledWith({ padMap: seeded }));
    expect(await screen.findByRole('heading', { name: 'Gamepad buttons' })).toBeInTheDocument();
    for (const name of ['A / ✕', 'LT / L2', 'Start', 'Left stick press / L3', 'D-pad up ↑']) {
      expect(screen.getByRole('combobox', { name: `${name} presses` })).toBeInTheDocument();
    }
    expect(screen.getAllByRole('combobox', { name: /presses$/ })).toHaveLength(16);
    // The four face rows are locked while the custom map is on.
    expect(screen.getAllByText('Set by the custom button map below.')).toHaveLength(4);
  });

  it('seeds a modern layout map from the modern preset', async () => {
    const { user, patch } = await openControls();
    await choose(user, 'Gamepad layout', 'Modern (DMC5 style)');
    await choose(user, 'Gamepad layout', 'Custom (every button)');
    const seeded = serializePadMap(presetLayout(true, ['cross', 'triangle', 'circle', 'square']));
    // The debounce may merge the layout change and the map into one patch.
    await waitFor(() => expect(patch).toHaveBeenCalledWith(expect.objectContaining({ padMap: seeded })));
  });

  it('warns about two buttons on one target and refuses to take Start away', async () => {
    const { user, patch } = await openControls();
    await choose(user, 'Gamepad layout', 'Custom (every button)');
    const rows = await screen.findByRole('heading', { name: 'Gamepad buttons' });
    expect(rows).toBeInTheDocument();
    // B also presses Cross: allowed, with a warning.
    await user.click(screen.getByRole('combobox', { name: 'B / ○ presses' }));
    await user.click(within(screen.getByRole('listbox')).getByRole('option', { name: /Cross$/ }));
    expect(await screen.findByText('Two buttons press the same PS2 button')).toBeInTheDocument();
    // Start is the only button pressing start: moving it away is refused and explained.
    // The change above reaches the backend after the debounce. Wait for THAT patch (the one that carries `b=cross`), not for any
    // patch: the earlier switch to the custom layout is debounced too, and on a slow machine it can fire first, leaving this
    // one to arrive after `mockClear` and be mistaken for a patch made by the refused change below.
    await waitFor(() => expect(patch).toHaveBeenCalledWith(expect.objectContaining({ padMap: expect.stringContaining('b=cross') })));
    patch.mockClear();
    await user.click(screen.getByRole('combobox', { name: 'Start presses' }));
    await user.click(within(screen.getByRole('listbox')).getByRole('option', { name: /Cross$/ }));
    expect(await screen.findByText(/Start must stay on one of the buttons/)).toBeInTheDocument();
    await new Promise((resolve) => setTimeout(resolve, 400));
    expect(patch).not.toHaveBeenCalled();
  });

  it('goes back to the layout with Back to a layout', async () => {
    const { user, patch } = await openControls();
    await choose(user, 'Gamepad layout', 'Custom (every button)');
    await screen.findByRole('heading', { name: 'Gamepad buttons' });
    await user.click(screen.getByRole('button', { name: 'Back to a layout' }));
    await waitFor(() => expect(screen.queryByRole('heading', { name: 'Gamepad buttons' })).not.toBeInTheDocument());
    await waitFor(() => expect(patch).toHaveBeenLastCalledWith({ padMap: '' }));
    expect(screen.getByRole('combobox', { name: 'Gamepad layout' })).toHaveTextContent('Classic (PS2)');
  });
});

describe('controller picker', () => {
  const padGuid = '0300dddd5e040000a102000000010000';

  it('lists what the game saw and marks the device that has no button layout', async () => {
    const { user } = await openControls();
    await user.click(await screen.findByRole('combobox', { name: 'Controller' }));
    const options = within(screen.getByRole('listbox')).getAllByRole('option').map((option) => option.textContent);
    expect(options).toEqual(['Automatic', '2.4G Consumer Control', '2.4G Keyboard', 'Generic X-Box pad (no button layout)']);
  });

  it('saves the chosen device with its name, warns about one with no layout, and goes back to automatic', async () => {
    const { user, patch } = await openControls();
    expect(screen.queryByText('The system has no button layout for this device')).not.toBeInTheDocument();
    await choose(user, 'Controller', 'Generic X-Box pad (no button layout)');
    await waitFor(() => expect(patch).toHaveBeenCalledWith(expect.objectContaining({ padDevice: padGuid, padDeviceName: 'Generic X-Box pad' })));
    expect(await screen.findByText('The system has no button layout for this device')).toBeInTheDocument();
    await choose(user, 'Controller', 'Automatic');
    await waitFor(() => expect(patch).toHaveBeenLastCalledWith(expect.objectContaining({ padDevice: '', padDeviceName: '' })));
    await waitFor(() => expect(screen.queryByText('The system has no button layout for this device')).not.toBeInTheDocument());
  });

  it('says so when the game has not listed anything yet', async () => {
    await openControls(null);
    expect(await screen.findByText(/No controller has been seen yet/)).toBeInTheDocument();
    expect(screen.getByRole('combobox', { name: 'Controller' })).toHaveTextContent('Automatic');
  });

  it('keeps a chosen device that is not in the last list, and tells when the game could not find it', async () => {
    const { user, patch } = await openControls();
    await choose(user, 'Controller', '2.4G Keyboard');
    await waitFor(() => expect(patch).toHaveBeenCalled());
    // The game ran again without it plugged in.
    vi.spyOn(api, 'padDevices').mockResolvedValue({ ...dongleDevices, choice: 'absent', devices: dongleDevices.devices.filter((d) => d.name !== '2.4G Keyboard') });
    await user.click(screen.getByRole('button', { name: 'Refresh' }));
    expect(await screen.findByText(/did not connect|was not connected the last time/)).toBeInTheDocument();
    expect(screen.getByRole('combobox', { name: 'Controller' })).toHaveTextContent('2.4G Keyboard (not seen last time)');
  });
});

describe('mapping a controller the system has no layout for', () => {
  const padGuid = '0300dddd5e040000a102000000010000';
  const idle = (): LiveDevice => ({ index: 3, guid: padGuid, buttons: [], hats: [], axes: [] });

  /** What the game reports for the pad: the test changes it as a player would press things. */
  function feed(initial: LiveDevice, ageMs = 100) {
    const state = { device: initial, ageMs };
    vi.spyOn(api, 'padLive').mockImplementation(async () => ({ beat: 1, ageMs: state.ageMs, devices: [state.device] }));
    return state;
  }

  it('asks the game for the live feed while it is open and stops when it closes', async () => {
    api = createMockApi({ ready: true });
    setApi(api);
    const spy = vi.spyOn(api, 'padProbe');
    render(<App />);
    const user = userEvent.setup();
    await user.click(await screen.findByRole('button', { name: 'Controls' }));
    await choose(user, 'Controller', 'Generic X-Box pad (no button layout)');
    await user.click(await screen.findByRole('button', { name: 'Map its buttons' }));
    const dialog = await screen.findByRole('dialog');
    await waitFor(() => expect(spy).toHaveBeenCalledWith(true));
    // The game has not written anything: it is not running. Listening is not offered, saving is not possible.
    expect(within(dialog).getByText(/Waiting for the game/)).toBeInTheDocument();
    expect(within(dialog).getByRole('button', { name: 'Listen A' })).toBeDisabled();
    expect(within(dialog).getByRole('button', { name: 'Save' })).toBeDisabled();
    await user.click(within(dialog).getByRole('button', { name: 'Cancel' }));
    expect(spy).toHaveBeenLastCalledWith(false);
    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
  });

  it('builds a mapping from what the game reports and saves it', async () => {
    const { user, patch } = await openControls();
    await choose(user, 'Controller', 'Generic X-Box pad (no button layout)');
    const state = feed(idle());
    await user.click(await screen.findByRole('button', { name: 'Map its buttons' }));
    const dialog = await screen.findByRole('dialog');
    await within(dialog).findByText('Receiving input from the game.');
    const listen = async (name: string, press: Partial<LiveDevice>) => {
      await user.click(within(dialog).getByRole('button', { name: `Listen ${name}` }));
      await within(dialog).findByText(/Press or move/);
      state.device = { ...idle(), ...press };
      await waitFor(() => expect(within(dialog).queryByText(/Press or move/)).not.toBeInTheDocument());
      state.device = idle(); // released
    };
    expect(within(dialog).getByRole('button', { name: 'Save' })).toBeDisabled();
    await listen('A', { buttons: [0] });
    // Still no start: saving stays off and the dialog says why.
    expect(within(dialog).getByRole('button', { name: 'Save' })).toBeDisabled();
    expect(within(dialog).getByText(/Map Start before saving/)).toBeInTheDocument();
    await listen('Start', { buttons: [7] });
    await listen('D-pad up', { hats: [[0, 1]] });
    await listen('Left stick, push right', { axes: [[0, 150]] });
    await listen('LT (left trigger)', { axes: [[2, 200]] });
    expect(within(dialog).getByText('Hat 0, up')).toBeInTheDocument();
    expect(within(dialog).getByText('Button 7')).toBeInTheDocument();
    const save = within(dialog).getByRole('button', { name: 'Save' });
    expect(save).toBeEnabled();
    await user.click(save);
    await waitFor(() => expect(patch).toHaveBeenCalledWith(expect.objectContaining({ padSdlMap: 'a:b0,start:b7,dpup:h0.1,leftx:a0,lefttrigger:a2' })));
    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
    // The saved mapping is announced and can be changed or removed.
    expect(await screen.findByText(/hand-made button layout is set/)).toBeInTheDocument();
  });

  it('ignores a press that cannot be the input asked for, and what was already held when Listen was pressed', async () => {
    const { user } = await openControls();
    await choose(user, 'Controller', 'Generic X-Box pad (no button layout)');
    const state = feed({ ...idle(), buttons: [4] }); // a button already held
    await user.click(await screen.findByRole('button', { name: 'Map its buttons' }));
    const dialog = await screen.findByRole('dialog');
    await within(dialog).findByText('Receiving input from the game.');
    await user.click(within(dialog).getByRole('button', { name: 'Listen Left stick, push right' }));
    // A button is not a stick axis: it is ignored and the dialog keeps listening.
    state.device = { ...idle(), buttons: [4, 5] };
    await new Promise((resolve) => setTimeout(resolve, 400));
    expect(within(dialog).getByText(/Press or move/)).toBeInTheDocument();
    expect(within(dialog).queryByText('Button 5')).not.toBeInTheDocument();
    // Esc stops listening without changing anything.
    await user.keyboard('{Escape}');
    await waitFor(() => expect(within(dialog).queryByText(/Press or move/)).not.toBeInTheDocument());
    expect(within(dialog).getAllByText('Not mapped').length).toBeGreaterThan(10);
  });

  it('says so when the game is running but does not list the controller, and when it stopped writing', async () => {
    const { user } = await openControls();
    await choose(user, 'Controller', 'Generic X-Box pad (no button layout)');
    vi.spyOn(api, 'padLive').mockImplementation(async () => ({ beat: 1, ageMs: 100, devices: [{ ...idle(), guid: '0300eeee5e040000a102000000010000' }] }));
    await user.click(await screen.findByRole('button', { name: 'Map its buttons' }));
    const dialog = await screen.findByRole('dialog');
    expect(await within(dialog).findByText(/does not list this controller/)).toBeInTheDocument();
    feed(idle(), 9000); // written long ago: the game is gone
    expect(await within(dialog).findByText(/Waiting for the game/)).toBeInTheDocument();
  });
});
