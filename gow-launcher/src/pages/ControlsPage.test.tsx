import { cleanup, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import App from '../App';
import { createMockApi, setApi, type MockApi } from '../lib/backend';
import { serializePadMap, presetLayout } from '../lib/padmap';

let api: MockApi;

afterEach(cleanup);

async function openControls() {
  api = createMockApi({ ready: true });
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
    await waitFor(() => expect(patch).toHaveBeenCalled()); // the change above reaches the backend after the debounce
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
