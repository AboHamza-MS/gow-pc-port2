import { act, cleanup, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import App from '../App';
import { createMockApi, setApi, type MockApi } from '../lib/backend';

let api: MockApi;

afterEach(cleanup);

async function openOther() {
  api = createMockApi({ ready: true });
  setApi(api);
  render(<App />);
  const user = userEvent.setup();
  await user.click(await screen.findByRole('button', { name: 'Other' }));
  return user;
}

describe('session report in the Other tab', () => {
  it('shows the export row and the marker row', async () => {
    await openOther();
    expect(screen.getByRole('button', { name: 'Export report' })).toBeEnabled();
    expect(screen.getByRole('textbox', { name: 'Mark a problem' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Mark' })).toBeDisabled();
  });

  it('exports the report and says where it went', async () => {
    const user = await openOther();
    const exported = vi.spyOn(api, 'reportExport');
    await user.click(screen.getByRole('button', { name: 'Export report' }));
    expect(exported).toHaveBeenCalledTimes(1);
    expect(await screen.findByText('Report saved: gow-report-20260101-120000.zip')).toBeInTheDocument();
  });

  it('says so when the export fails', async () => {
    const user = await openOther();
    vi.spyOn(api, 'reportExport').mockRejectedValue(new Error('disk full'));
    await user.click(screen.getByRole('button', { name: 'Export report' }));
    expect(await screen.findByText('The report could not be saved.')).toBeInTheDocument();
  });

  it('sends a note and empties the field', async () => {
    const user = await openOther();
    const mark = vi.spyOn(api, 'reportMark');
    const field = screen.getByRole('textbox', { name: 'Mark a problem' });
    await user.type(field, 'fell through the floor');
    await user.click(screen.getByRole('button', { name: 'Mark' }));
    expect(mark).toHaveBeenCalledWith('fell through the floor');
    await waitFor(() => expect(field).toHaveValue(''));
    expect(await screen.findByText('Marked for the next report.')).toBeInTheDocument();
  });
});

describe('session report in the failure dialog', () => {
  it('offers the export and the note next to the log', async () => {
    api = createMockApi({ ready: true });
    setApi(api);
    render(<App />);
    await screen.findByRole('button', { name: 'PLAY' });
    act(() => api.emit('game-finished', { code: 3, failed: true, seconds: 3, summary: 'boom', failureKind: null }));
    const dialog = await screen.findByRole('dialog');
    expect(within(dialog).getByRole('button', { name: 'Open log' })).toBeInTheDocument();
    expect(within(dialog).getByRole('textbox', { name: 'Mark a problem' })).toBeInTheDocument();
    const exported = vi.spyOn(api, 'reportExport');
    await userEvent.setup().click(within(dialog).getByRole('button', { name: 'Export report' }));
    expect(exported).toHaveBeenCalledTimes(1);
  });
});
