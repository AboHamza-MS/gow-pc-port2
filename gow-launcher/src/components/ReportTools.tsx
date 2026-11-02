import { useState } from 'react';
import { useApp } from '../state/app';
import { SecondaryButton, TextInput } from './ui';

/** The session report: the export (the ZIP to attach to a bug report) and the notes about a problem. */
export function useReportExport() {
  const { api, notify, t, f } = useApp();
  const [busy, setBusy] = useState(false);
  const run = async () => {
    if (busy) return;
    setBusy(true);
    try {
      notify(f('reportSaved', await api.reportExport()));
    } catch {
      notify(t('reportFailed'));
    } finally {
      setBusy(false);
    }
  };
  return { busy, run };
}

/** A text field and a button that store a note for the next report. */
export function MarkField() {
  const { api, notify, t } = useApp();
  const [text, setText] = useState('');
  const save = async () => {
    if (!text.trim()) return;
    try {
      if (await api.reportMark(text)) {
        setText('');
        notify(t('reportMarked'));
      }
    } catch {
      notify(t('reportFailed'));
    }
  };
  return (
    <div className="flex w-full gap-2">
      <TextInput
        aria-label={t('reportMarkTitle')}
        placeholder={t('reportMarkPlaceholder')}
        maxLength={200}
        value={text}
        onChange={(event) => setText(event.target.value)}
        onKeyDown={(event) => event.key === 'Enter' && void save()}
      />
      <SecondaryButton disabled={!text.trim()} onClick={() => void save()}>
        {t('reportMarkButton')}
      </SecondaryButton>
    </div>
  );
}
