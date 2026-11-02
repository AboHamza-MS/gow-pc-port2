import { cleanup, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import App from '../App';
import { createMockApi, setApi } from '../lib/backend';

afterEach(cleanup);

async function openGraphics() {
  const api = createMockApi({ ready: true });
  setApi(api);
  render(<App />);
  const user = userEvent.setup();
  const patch = vi.spyOn(api, 'patchSettings');
  const preset = await screen.findByRole('combobox', { name: 'Graphics preset' });
  return { user, patch, preset };
}

describe('graphics presets in the Graphics tab', () => {
  it('shows Balanced on the defaults and writes only its four options when Quality is chosen', async () => {
    const { user, patch, preset } = await openGraphics();
    expect(preset).toHaveTextContent('Balanced');
    expect(screen.queryByText('Above 2× is heavy')).not.toBeInTheDocument();
    await user.click(preset);
    await user.click(screen.getByRole('option', { name: /^Quality/ }));
    // The change reaches the backend after the debounce, as one patch with the four options and nothing else.
    await waitFor(() => expect(patch).toHaveBeenCalledWith({ internalResolution: '3', filter: 'fsr', sharpness: 0.8, fxaa: true }));
    // The heavy-scale warning appears with the 3× scale, and the preset still reads as Quality.
    expect(await screen.findByText('Above 2× is heavy')).toBeInTheDocument();
    expect(screen.getByRole('combobox', { name: 'Graphics preset' })).toHaveTextContent('Quality');
  });

  it('turns into Custom after a manual change, and Custom cannot be picked', async () => {
    const { user, preset } = await openGraphics();
    await user.click(screen.getByRole('switch', { name: 'Anti-aliasing (FXAA)' }));
    expect(await screen.findByRole('combobox', { name: 'Graphics preset' })).toHaveTextContent('Custom');
    await user.click(preset);
    expect(screen.getByRole('option', { name: 'Custom' })).toHaveAttribute('aria-disabled', 'true');
    await user.click(screen.getByRole('option', { name: /^Performance/ }));
    expect(await screen.findByRole('combobox', { name: 'Graphics preset' })).toHaveTextContent('Performance');
  });
});
