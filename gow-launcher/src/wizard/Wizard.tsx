import { useCallback, useEffect, useRef, useState, type ReactNode } from 'react';
import { Check, FileArchive, FileCheck, FileDiff, FolderOpen, Languages, Loader2, Sparkles, Trophy } from 'lucide-react';
import { Modal } from '../components/Shell';
import { CheckDot, PrimaryButton, ProgressBar, SecondaryButton, TextInput } from '../components/ui';
import type { ChdError, ChdOutcome, ChdPlan, DestinationValidation, InstallOutcome, InstallProgress, IsoValidation, PatchError, PatchOutcome, WizardInfo } from '../lib/types';
import { useApp } from '../state/app';
import { destinationMessageKey, fileName, isChd, isoMessageKey, overallProgress, stageWeights } from './progress';

// 0 ISO, 1 location, 2 components (only if something is missing), 3 summary, 4 installing, 5 success, 6 error.
type Step = 0 | 1 | 2 | 3 | 4 | 5 | 6;

const emptyIso: IsoValidation = { ok: false, reason: 'empty', hash: '' };
const emptyDestination: DestinationValidation = { ok: false, reason: 'empty', freeGb: 0 };
const noInfo: WizardInfo = {
  environment: { vs: true, cmakeUnusable: false, git: true, installer: true, missing: [], missingPackages: [], installCommand: '', cpuMissing: [], kitMissing: [] },
  hasManifest: false, defaultDestination: '',
};

function Body({ children }: { children: ReactNode }) {
  return <div className="flex min-h-[330px] flex-col gap-3">{children}</div>;
}

const Lead = ({ children }: { children: ReactNode }) => <p className="text-sm leading-[1.25] text-ink-soft">{children}</p>;

function Component({ title, description, size, checked, onChange }: {
  title: string; description: string; size: string; checked: boolean; onChange: (value: boolean) => void;
}) {
  return (
    <label className="gow-row flex cursor-pointer items-start gap-3.5 py-3 pl-[22px] pr-[18px]">
      <input type="checkbox" checked={checked} onChange={(event) => onChange(event.target.checked)} className="peer sr-only" />
      <span className="mt-0.5 flex h-5 w-5 shrink-0 items-center justify-center rounded-[4px] border-2 border-[#5f4326] bg-[#efdfc0] text-cream shadow-[inset_0_1px_2px_rgba(60,35,10,0.35)] peer-checked:border-[#5a0f08] peer-checked:bg-accent peer-focus-visible:ring-2 peer-focus-visible:ring-accent/50 [&>svg]:opacity-0 peer-checked:[&>svg]:opacity-100">
        <Check size={14} strokeWidth={3.5} />
      </span>
      <span className="flex-1">
        <span className="gow-text-shadow block text-[14.5px] font-bold leading-tight text-ink">{title}</span>
        <span className="mt-0.5 block text-[13px] font-medium leading-[1.25] text-[#47321f]">{description}</span>
      </span>
      <span className="text-[13px] font-semibold text-ink-soft">{size}</span>
    </label>
  );
}

export function Wizard() {
  const { api, t, f, lang, duration, closeWizard, settings, adoptSettings, notify } = useApp();
  const [step, setStep] = useState<Step>(0);
  const [info, setInfo] = useState<WizardInfo>(noInfo);
  const [iso, setIso] = useState('');
  const [automaticIso, setAutomaticIso] = useState(false);
  const [searching, setSearching] = useState(false);
  const [isoCheck, setIsoCheck] = useState<IsoValidation>(emptyIso);
  const [destination, setDestination] = useState('');
  const [destinationCheck, setDestinationCheck] = useState<DestinationValidation>(emptyDestination);
  const [include, setInclude] = useState({ vs: true, git: true });
  const [vu1MemoryLimitGb, setVu1MemoryLimitGb] = useState<4 | 8>(4);
  const [progress, setProgress] = useState(0);
  const [stage, setStage] = useState(-1);
  const [seconds, setSeconds] = useState(0);
  const [confirmingCancel, setConfirmingCancel] = useState(false);
  const [cancellation, setCancellation] = useState('');
  const [errorText, setErrorText] = useState('');
  const [errorHint, setErrorHint] = useState('');
  const [hovering, setHovering] = useState(false);
  const [rechecking, setRechecking] = useState(false);
  // GOW-Port (0.3.0): a translation patch applied to the original ISO (only offered for the original).
  const [patchWanted, setPatchWanted] = useState(false);
  const [patchPath, setPatchPath] = useState('');
  const [patchProblem, setPatchProblem] = useState<PatchError | null>(null);
  const [patchFraction, setPatchFraction] = useState(-1);
  const [patched, setPatched] = useState<{ original: string; path: string } | null>(null);
  const patchingFrom = useRef('');
  const patching = patchFraction >= 0;
  // GOW-Port (0.3.1): a CHD is validated in place, then converted to an ISO next to it before going on.
  const [chdPlan, setChdPlan] = useState<ChdPlan | null>(null);
  const [chdProblem, setChdProblem] = useState<ChdError | null>(null);
  const [chdFraction, setChdFraction] = useState(-1);
  const [converted, setConverted] = useState<{ chd: string; path: string } | null>(null);
  const convertingFrom = useRef('');
  const converting = chdFraction >= 0;
  const chosenChd = isChd(iso);
  const outcome = useRef<InstallOutcome | null>(null);
  const live = useRef({ step, isoOk: false, weights: stageWeights(false) });
  live.current = { step, isoOk: isoCheck.ok, weights: stageWeights(info.hasManifest) };

  const { environment } = info;
  // Linux: the installer cannot install system packages by itself (it needs the user's password), so the wizard lists
  // them and waits until a new check finds none missing.
  const manual = environment.missing.length > 0;
  const cpuSupported = environment.cpuMissing.length === 0;
  // Only a missing Visual Studio (the C++ compiler) is offered. A Visual Studio without a usable CMake or Ninja needs nothing
  // here: the installer downloads portable ones by itself (environment.cmakeUnusable is informational).
  const needVs = !environment.vs;
  const missing = needVs || !environment.git;
  const componentsReady = !manual && (environment.vs || include.vs) && (environment.git || include.git);
  const indicator = missing ? [0, 1, 2, 3] : [0, 1, 3];

  const validateIso = useCallback(
    async (path: string) => setIsoCheck(path.trim().length === 0 ? emptyIso : await api.validateIso(path)),
    [api],
  );
  const recheck = async () => {
    setRechecking(true);
    try {
      setInfo(await api.wizardInfo());
    } finally {
      setRechecking(false);
    }
  };
  const validateDestination = useCallback(async (path: string) => setDestinationCheck(await api.validateDestination(path)), [api]);

  // Opening: environment, default folder, and the ISO already configured or found next to the game.
  useEffect(() => {
    let cancelled = false;
    void (async () => {
      const wizard = await api.wizardInfo();
      if (cancelled) return;
      setInfo(wizard);
      setDestination(wizard.defaultDestination);
      void validateDestination(wizard.defaultDestination);
      const start = settings.iso.length > 0 ? settings.iso : await api.detectedIso();
      if (cancelled) return;
      setIso(start);
      const check = start ? await api.validateIso(start) : emptyIso;
      if (cancelled) return;
      setIsoCheck(check);
      if (!check.ok) void api.findIso();
    })();
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps -- runs once, on opening
  }, []);

  useEffect(() => {
    const stops = [
      api.on('iso-searching', setSearching),
      api.on('iso-found', (path) => {
        if (live.current.isoOk || live.current.step !== 0) return;
        setIso(path);
        setAutomaticIso(true);
        void validateIso(path);
      }),
      api.on('install-progress', (event: InstallProgress) => {
        setStage(event.stage);
        // The bar only moves forward: the optimization stage rebuilds and resets ninja's counter.
        setProgress((previous) => Math.max(previous, overallProgress(event.stage, event.stageProgress, live.current.weights)));
      }),
      api.on('install-done', (event: InstallOutcome) => {
        setConfirmingCancel(false);
        outcome.current = event;
        if (event.cancelled) {
          setCancellation(t('wizCancelled'));
          setStep(3);
        } else if (event.ok) {
          setProgress(1);
          setStep(5);
        } else {
          setErrorText(event.error);
          setErrorHint(event.failureKind ? t(event.failureKind) : '');
          setStep(6);
        }
      }),
      api.on('iso-patch-progress', (fraction: number) => setPatchFraction((before) => (before < 0 ? before : Math.max(before, fraction)))),
      api.on('iso-patch-done', (result: PatchOutcome) => {
        setPatchFraction(-1);
        if (result.ok && result.validation) {
          setPatched({ original: patchingFrom.current, path: result.path });
          setPatchProblem(null);
          setIso(result.path);
          setAutomaticIso(false);
          setIsoCheck(result.validation);
        } else {
          setPatchProblem(result.error ?? 'write');
        }
      }),
      api.on('iso-chd-progress', (fraction: number) => setChdFraction((before) => (before < 0 ? before : Math.max(before, fraction)))),
      api.on('iso-chd-done', (result: ChdOutcome) => {
        setChdFraction(-1);
        if (result.ok && result.validation) {
          setConverted({ chd: convertingFrom.current, path: result.path });
          setChdProblem(null);
          setIso(result.path);
          setAutomaticIso(false);
          setIsoCheck(result.validation);
        } else {
          setChdProblem(result.error ?? 'write');
          if (result.validation) setIsoCheck(result.validation);
        }
      }),
      api.onFileDrop((paths, over) => {
        setHovering(over && live.current.step === 0);
        if (paths.length > 0 && live.current.step === 0) {
          setIso(paths[0]);
          setAutomaticIso(false);
          void validateIso(paths[0]);
        }
      }),
    ];
    return () => stops.forEach((stop) => void stop.then((off) => off()));
  }, [api, t, validateIso]);

  // Another ISO than the translated one drops the patch's result; closing the wizard stops a patch being applied.
  useEffect(() => {
    if (patched && iso !== patched.path) setPatched(null);
  }, [iso, patched]);
  const patchingNow = useRef(false);
  patchingNow.current = patching;
  useEffect(() => () => {
    if (patchingNow.current) void api.isoPatchCancel();
  }, [api]);

  // A valid CHD: where its ISO will go. Another file than the converted ISO drops the conversion's result; closing the
  // wizard stops a conversion.
  useEffect(() => {
    if (converted && iso !== converted.path) setConverted(null);
  }, [iso, converted]);
  useEffect(() => setChdProblem(null), [iso]);
  useEffect(() => {
    let current = true;
    if (!chosenChd || !isoCheck.ok) {
      setChdPlan(null);
      return;
    }
    void api.isoChdPlan(iso).then((plan) => {
      if (current) setChdPlan(plan);
    });
    return () => {
      current = false;
    };
  }, [api, iso, chosenChd, isoCheck.ok]);
  const convertingNow = useRef(false);
  convertingNow.current = converting;
  useEffect(() => () => {
    if (convertingNow.current) void api.isoChdCancel();
  }, [api]);

  useEffect(() => {
    if (step !== 4) return;
    const timer = window.setInterval(() => setSeconds((s) => s + 1), 1000);
    return () => window.clearInterval(timer);
  }, [step]);

  // The ISO field validates a moment after typing stops.
  const typing = useRef<number>(0);
  const typeIso = (value: string) => {
    setIso(value);
    setAutomaticIso(false);
    window.clearTimeout(typing.current);
    typing.current = window.setTimeout(() => void validateIso(value), 350);
  };
  const typeDestination = (value: string) => {
    setDestination(value);
    void validateDestination(value);
  };

  const canAdvance = (() => {
    switch (step) {
      case 0: return isoCheck.ok && cpuSupported && !patching && !(patchWanted && !patched) && !chosenChd && !converting;
      case 1: return destinationCheck.ok && environment.installer && environment.kitMissing.length === 0;
      case 2: return componentsReady;
      case 3: case 5: case 6: return true;
      default: return false;
    }
  })();

  const compile = async () => {
    setErrorText('');
    setErrorHint('');
    setCancellation('');
    setConfirmingCancel(false);
    setProgress(0);
    setStage(-1);
    setSeconds(0);
    outcome.current = null;
    setStep(4);
    try {
      await api.installStart({ iso, destination, installTools: !manual && needVs && include.vs, vu1MemoryLimitGb });
    } catch (failure) {
      setErrorText(String(failure));
      setStep(6);
    }
  };

  const complete = async () => {
    const done = outcome.current;
    if (!done) return closeWizard();
    adoptSettings(await api.installComplete({ destination, elf: done.elf, executable: done.executable }));
    notify(t('ready'));
    closeWizard();
  };

  const next = () => {
    if (!canAdvance) return;
    if (step === 0) setStep(1);
    else if (step === 1) setStep(missing ? 2 : 3);
    else if (step === 2) setStep(3);
    else if (step === 3) void compile();
    else if (step === 5) void complete();
    else if (step === 6) void compile();
  };
  const back = () => {
    if (step === 1) setStep(0);
    else if (step === 2) setStep(1);
    else if (step === 3) setStep(missing ? 2 : 1);
  };

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === 'Enter' && !(event.target instanceof HTMLButtonElement)) next();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  });

  const browseIso = async () => {
    const chosen = await api.pick({
      title: t('wizChooseIso'),
      filters: [{ name: 'ISO / CHD', extensions: ['iso', 'ISO', 'chd', 'CHD'] }, { name: '*', extensions: ['*'] }],
    });
    if (chosen) {
      setIso(chosen);
      setAutomaticIso(false);
      void validateIso(chosen);
    }
  };
  const isOriginal = isoCheck.ok && !isoCheck.translation && !patched && !chosenChd;
  const convertChd = async () => {
    if (!chdPlan || converting || (!chdPlan.enoughSpace && !chdPlan.existing)) return;
    convertingFrom.current = iso;
    setChdProblem(null);
    setChdFraction(0);
    try {
      await api.isoChdStart(iso);
    } catch {
      setChdFraction(-1);
      setChdProblem('write');
    }
  };
  const gb = (value: number) => value.toLocaleString(lang, { minimumFractionDigits: 1, maximumFractionDigits: 1 });
  const chdMessage = () => {
    if (chdProblem) return t(`wizChdError_${chdProblem}`);
    if (converting) return f('wizChdProgress', Math.min(100, Math.round(chdFraction * 100)));
    if (!chdPlan) return t('wizChdReading');
    if (chdPlan.existing) return f('wizChdExisting', fileName(chdPlan.output));
    if (!chdPlan.enoughSpace) return f('wizChdSpace', gb(chdPlan.isoGb + 0.25), gb(chdPlan.freeGb));
    return f('wizChdWillCreate', fileName(chdPlan.output), gb(chdPlan.isoGb));
  };
  const choosePatch = async () => {
    const chosen = await api.pick({
      title: t('wizPatchChooseTitle'),
      filters: [{ name: 'xdelta', extensions: ['xdelta', 'vcdiff', 'xd3', 'delta'] }, { name: '*', extensions: ['*'] }],
    });
    if (!chosen) return;
    setPatchPath(chosen);
    setPatchProblem(await api.isoPatchCheck(chosen));
  };
  const applyPatch = async () => {
    if (!patchPath || patchProblem || patching) return;
    patchingFrom.current = iso;
    setPatchProblem(null);
    setPatchFraction(0);
    try {
      await api.isoPatchStart(iso, patchPath);
    } catch {
      setPatchFraction(-1);
      setPatchProblem('write');
    }
  };
  const useOriginal = () => {
    if (!patched) return;
    const original = patched.original;
    setPatched(null);
    setPatchWanted(false);
    setIso(original);
    setAutomaticIso(false);
    void validateIso(original);
  };
  const patchOutput = patchPath ? `${fileName(patchPath).replace(/\.[^.]*$/, '')}.iso` : '';
  const browseFolder = async () => {
    const chosen = await api.pick({ title: t('wizChooseFolder'), directory: true, defaultPath: destination || undefined });
    if (chosen) typeDestination(chosen);
  };

  const isoMessage = () => {
    if (isoCheck.ok && isoCheck.translation) return t('wizIsoTranslation');
    if (isoCheck.ok && chosenChd) return automaticIso ? t('wizChdFound') : t('wizChdOk');
    if (isoCheck.ok) return automaticIso ? t('wizIsoFound') : t('wizIsoOk');
    if (!iso) return searching ? t('wizSearchingHint') : t('wizDragHint');
    const key = isoCheck.reason ? isoMessageKey[isoCheck.reason] : '';
    if (key === 'wizIsoOtherEdition') return f(key, isoCheck.edition ?? '');
    return key === 'wizIsoModified' ? f(key, isoCheck.hash) : key ? t(key) : '';
  };
  const destinationMessage = () => {
    if (destinationCheck.ok) return f('wizSpaceFree', destinationCheck.freeGb.toFixed(0));
    const key = destinationCheck.reason ? destinationMessageKey[destinationCheck.reason] : 'wizDestinationEmpty';
    return key === 'wizDestinationSpace' ? f(key, destinationCheck.freeGb.toFixed(0)) : t(key);
  };
  const components = [
    !manual && needVs && include.vs && t('wizCompVs'),
    !environment.git && include.git && t('wizCompGit'),
  ].filter(Boolean);

  const titles = [t('wizStep1Title'), t('wizStep2Title'), t('wizStep3Title'), t('wizStep4Title'), t('wizCompiling'), t('wizSuccessTitle'), t('wizErrorTitle')];
  const indicatorLabels = [t('wizStepIso'), t('wizStepLocal'), t('wizStepComp'), t('wizStepInstall')];
  const stageCount = info.hasManifest ? 6 : 7;
  const currentStage = Math.max(0, stage);

  return (
    <Modal width={720}>
      <div className="flex flex-col gap-4">
        <div className="flex items-center gap-3">
          {step === 5 ? <Trophy size={22} className="text-gold-light" /> : <Sparkles size={22} className={step === 6 ? 'text-bad' : 'text-gold-light'} />}
          <h2 className="gow-text-shadow flex-1 text-[20px] font-bold uppercase tracking-wide text-ink">{titles[step]}</h2>
        </div>

        {step <= 3 && (
          <div className="flex items-center gap-2">
            {indicator.map((stepId, index) => {
              const done = stepId < step;
              const current = stepId === step;
              return (
                <div key={stepId} className={`flex items-center gap-2 ${index < indicator.length - 1 ? 'flex-1' : ''}`}>
                  <span
                    className={`flex h-6 w-6 shrink-0 items-center justify-center rounded-full border text-xs font-bold ${
                      done ? 'border-gold-light bg-gold text-cream' : current ? 'border-gold-light text-gold-light' : 'border-line text-ink-muted'
                    }`}
                  >
                    {done ? <Check size={13} strokeWidth={3} /> : index + 1}
                  </span>
                  <span className={`text-[13px] ${current ? 'font-semibold text-ink' : 'text-ink-muted'}`}>{indicatorLabels[stepId]}</span>
                  {index < indicator.length - 1 && <span className={`h-px flex-1 ${done ? 'bg-gold-dark' : 'bg-line'}`} />}
                </div>
              );
            })}
          </div>
        )}
        <div className="h-px bg-line" />

        {step === 0 && (
          <Body>
            <Lead>{t('wizStep1Text')}</Lead>
            <div
              className={`gow-card flex h-[104px] items-center gap-4 rounded-lg px-[26px] transition-shadow ${
                hovering ? 'shadow-[0_0_0_3px_rgba(163,25,15,0.55)]' : ''
              }`}
            >
              <span
                className={`flex h-12 w-12 shrink-0 items-center justify-center rounded-full border ${
                  isoCheck.ok ? 'border-gold-light bg-gold' : 'border-line'
                }`}
              >
                {searching && !isoCheck.ok ? (
                  <Loader2 size={26} className="animate-spin text-gold-light" />
                ) : isoCheck.ok ? (
                  <Check size={22} className="text-cream" strokeWidth={3} />
                ) : (
                  <FolderOpen size={22} className={iso ? 'text-bad' : 'text-ink-soft'} />
                )}
              </span>
              <div className="min-w-0 flex-1">
                <div className="truncate text-base font-medium text-ink">
                  {iso ? fileName(iso) : searching ? t('wizSearchingIso') : t('wizDragIso')}
                </div>
                <div className={`line-clamp-3 text-[13px] font-medium ${isoCheck.ok ? 'text-ok' : iso ? 'text-bad' : 'text-[#47321f]'}`}>{isoMessage()}</div>
              </div>
              <SecondaryButton onClick={() => void browseIso()}>{t('browse')}</SecondaryButton>
            </div>
            <TextInput value={iso} onChange={(event) => typeIso(event.target.value)} placeholder={t('wizIsoPlaceholder')} className="h-[38px]" />
            {chosenChd && (isoCheck.ok || chdProblem) && (
              <div className="flex flex-col gap-2.5">
                <div className="gow-card flex flex-col gap-2.5 py-3 pl-[22px] pr-[18px]">
                  <div className="flex items-center gap-3">
                    <FileArchive size={20} className={`shrink-0 ${chdProblem ? 'text-bad' : 'text-gold-light'}`} />
                    <div className="min-w-0 flex-1">
                      <div className="text-[14px] font-bold text-ink">{t('wizChdTitle')}</div>
                      <div className={`text-[13px] font-medium ${chdProblem || (chdPlan && !chdPlan.enoughSpace) ? 'text-bad' : 'text-[#47321f]'}`}>
                        {chdMessage()}
                      </div>
                    </div>
                    {converting ? (
                      <SecondaryButton onClick={() => void api.isoChdCancel()}>{t('wizChdCancel')}</SecondaryButton>
                    ) : (
                      <PrimaryButton
                        disabled={!isoCheck.ok || !chdPlan || (!chdPlan.enoughSpace && !chdPlan.existing)}
                        onClick={() => void convertChd()}
                      >
                        {chdPlan?.existing ? t('wizChdUseExisting') : t('wizChdConvert')}
                      </PrimaryButton>
                    )}
                  </div>
                  {converting && <ProgressBar label={t('wizChdProgressLabel')} fraction={chdFraction} />}
                </div>
                {!converting && <p className="text-[13px] text-ink-muted">{t('wizChdPending')}</p>}
              </div>
            )}
            {converted && !patched && (
              <div role="status" className="gow-card flex items-center gap-3.5 py-3 pl-[22px] pr-[18px]">
                <FileCheck size={20} className="shrink-0 text-gold-light" />
                <div className="min-w-0 flex-1">
                  <div className="truncate text-[14px] font-bold text-ink">{f('wizChdDone', fileName(converted.path))}</div>
                  <div className="text-[13px] font-medium text-[#47321f]">{t('wizChdKept')}</div>
                </div>
              </div>
            )}
            {patched ? (
              <div role="status" className="gow-card flex items-center gap-3.5 py-3 pl-[22px] pr-[18px]">
                <Languages size={20} className="shrink-0 text-gold-light" />
                <div className="min-w-0 flex-1">
                  <div className="truncate text-[14px] font-bold text-ink">{f('wizPatchDone', fileName(patched.path))}</div>
                  <div className="text-[13px] font-medium text-[#47321f]">{t('wizPatchOriginalKept')}</div>
                </div>
                <SecondaryButton onClick={useOriginal}>{t('wizPatchUseOriginal')}</SecondaryButton>
              </div>
            ) : (isOriginal || patching) && (
              <div className="flex flex-col gap-2.5">
                <label className={`gow-row flex items-start gap-3.5 py-3 pl-[22px] pr-[18px] ${patching ? 'opacity-60' : 'cursor-pointer'}`}>
                  <input
                    type="checkbox"
                    checked={patchWanted}
                    disabled={patching}
                    onChange={(event) => {
                      setPatchWanted(event.target.checked);
                      setPatchProblem(patchPath ? patchProblem : null);
                    }}
                    className="peer sr-only"
                  />
                  <span className="mt-0.5 flex h-5 w-5 shrink-0 items-center justify-center rounded-[4px] border-2 border-[#5f4326] bg-[#efdfc0] text-cream shadow-[inset_0_1px_2px_rgba(60,35,10,0.35)] peer-checked:border-[#5a0f08] peer-checked:bg-accent peer-focus-visible:ring-2 peer-focus-visible:ring-accent/50 [&>svg]:opacity-0 peer-checked:[&>svg]:opacity-100">
                    <Check size={14} strokeWidth={3.5} />
                  </span>
                  <span className="flex-1">
                    <span className="gow-text-shadow block text-[14.5px] font-bold leading-tight text-ink">{t('wizPatchTitle')}</span>
                    <span className="mt-0.5 block text-[13px] font-medium leading-[1.25] text-[#47321f]">{t('wizPatchDesc')}</span>
                  </span>
                </label>
                {patchWanted && (
                  <div className="gow-card flex flex-col gap-2.5 py-3 pl-[22px] pr-[18px]">
                    <div className="flex items-center gap-3">
                      <FileDiff size={20} className={`shrink-0 ${patchPath && !patchProblem ? 'text-gold-light' : 'text-ink-soft'}`} />
                      <div className="min-w-0 flex-1">
                        <div className="truncate text-[14px] font-semibold text-ink">{patchPath ? fileName(patchPath) : t('wizPatchNone')}</div>
                        <div className={`text-[13px] font-medium ${patchProblem ? 'text-bad' : 'text-[#47321f]'}`}>
                          {patchProblem
                            ? t(`wizPatchError_${patchProblem}`)
                            : patching
                              ? f('wizPatchProgress', Math.min(100, Math.round(patchFraction * 100)))
                              : patchPath
                                ? f('wizPatchWillCreate', patchOutput)
                                : t('wizPatchHint')}
                        </div>
                      </div>
                      {patching ? (
                        <SecondaryButton onClick={() => void api.isoPatchCancel()}>{t('wizPatchCancel')}</SecondaryButton>
                      ) : (
                        <>
                          <SecondaryButton onClick={() => void choosePatch()}>{t('wizPatchChoose')}</SecondaryButton>
                          <PrimaryButton disabled={!patchPath || (patchProblem !== null && patchProblem !== 'cancelled')} onClick={() => void applyPatch()}>
                            {t('wizPatchApply')}
                          </PrimaryButton>
                        </>
                      )}
                    </div>
                    {patching && <ProgressBar label={t('wizPatchProgressLabel')} fraction={patchFraction} />}
                  </div>
                )}
                {patchWanted && !patching && !patched && <p className="text-[13px] text-ink-muted">{t('wizPatchPending')}</p>}
              </div>
            )}
            {!cpuSupported && <p role="alert" className="text-[13px] font-semibold text-bad">{t('wizCpuUnsupported')}</p>}
          </Body>
        )}

        {step === 1 && (
          <Body>
            <Lead>{t('wizStep2Text')}</Lead>
            <div className="flex gap-2">
              <TextInput value={destination} onChange={(event) => typeDestination(event.target.value)} className="h-10 text-sm" />
              <SecondaryButton onClick={() => void browseFolder()}>{t('browse')}</SecondaryButton>
            </div>
            <div className="flex items-start gap-2">
              <span className={`mt-[5px] h-2 w-2 shrink-0 rounded-full ${destinationCheck.ok ? 'bg-ok' : 'bg-bad'}`} />
              <span className={`text-[13px] ${destinationCheck.ok ? 'text-ink-soft' : 'text-bad'}`}>{destinationMessage()}</span>
            </div>
            <p className="text-[13px] text-ink-muted">{t('wizStep2Hint')}</p>
            <div className="gow-card flex flex-col">
              <Component title={t('wizRamLimit4')} description={t('wizRamLimit4Desc')} size="4 GB" checked={vu1MemoryLimitGb === 4} onChange={(checked) => checked && setVu1MemoryLimitGb(4)} />
              <Component title={t('wizRamLimit8')} description={t('wizRamLimit8Desc')} size="8 GB" checked={vu1MemoryLimitGb === 8} onChange={(checked) => checked && setVu1MemoryLimitGb(8)} />
            </div>
            {environment.kitMissing.length > 0 ? (
              <p role="alert" className="text-[13px] text-bad">{f('failureKitMissing', environment.kitMissing.join(', '))}</p>
            ) : (
              !environment.installer && <p className="text-[13px] text-bad">{t('wizNoInstaller')}</p>
            )}
          </Body>
        )}

        {step === 2 && (
          <Body>
            {manual ? (
              <>
                <Lead>{t('wizLinuxMissingDesc')}</Lead>
                <p className="text-[13px] font-semibold text-ink">{f('wizLinuxMissingList', environment.missing.join(', '))}</p>
                {environment.installCommand ? (
                  <div className="gow-card flex items-center gap-3 py-3 pl-[22px] pr-[18px]">
                    <code className="min-w-0 flex-1 select-text break-all text-[13px] text-ink">{environment.installCommand}</code>
                    <SecondaryButton onClick={() => void navigator.clipboard?.writeText(environment.installCommand).catch(() => undefined)}>
                      {t('wizLinuxCopy')}
                    </SecondaryButton>
                  </div>
                ) : (
                  <p className="text-[13px] text-warn">{t('wizLinuxUnknown')}</p>
                )}
                <div>
                  <SecondaryButton onClick={() => void recheck()} disabled={rechecking}>{t('wizLinuxCheckAgain')}</SecondaryButton>
                </div>
              </>
            ) : (
              <>
                <Lead>{t('wizComponentsDesc')}</Lead>
                {needVs && <Component title={t('wizCompVs')} description={t('wizCompVsDesc')} size="~6 GB" checked={include.vs} onChange={(vs) => setInclude({ ...include, vs })} />}
                {!environment.git && <Component title={t('wizCompGit')} description={t('wizCompGitDesc')} size="~50 MB" checked={include.git} onChange={(git) => setInclude({ ...include, git })} />}
                {!componentsReady && <p className="text-[13px] text-warn">{t('wizComponentRequired')}</p>}
              </>
            )}
          </Body>
        )}

        {step === 3 && (
          <Body>
            <Lead>{t('wizSummaryText')}</Lead>
            <dl className="gow-card flex flex-col gap-[9px] py-4 pl-[26px] pr-[22px]">
              {[
                [t('wizSummaryIso'), fileName(iso)],
                [t('wizSummaryLocal'), destination],
                [t('wizSummaryComp'), components.length > 0 ? components.join(', ') : t('wizNone')],
                [t('wizSummarySpace'), t('wizSummarySpaceVal')],
                [t('wizSummaryTime'), t('wizSummaryTimeVal')],
                [t('wizSummaryRam'), `${vu1MemoryLimitGb} GB`],
              ].map(([label, value]) => (
                <div key={label as string} className="flex gap-3.5">
                  <dt className="w-[150px] shrink-0 text-[13px] font-semibold text-ink-soft">{label}</dt>
                  <dd className="min-w-0 flex-1 break-all text-sm text-ink">{value}</dd>
                </div>
              ))}
            </dl>
            {!info.hasManifest && <p className="text-[13px] text-warn">{t('wizOptimizationNotice')}</p>}
            {cancellation && <p className="text-[13px] text-warn">{cancellation}</p>}
          </Body>
        )}

        {step === 4 && (
          <Body>
            <Lead>{t('wizCompilingText')}</Lead>
            <div className="flex items-baseline justify-between">
              <span className="text-[22px] font-bold text-gold-light">{Math.round(progress * 100)} %</span>
              <span className="text-[13px] text-ink-muted">{f('wizTimeElapsed', duration(seconds))}</span>
            </div>
            <div className="gow-track h-[14px]" role="progressbar" aria-valuenow={Math.round(progress * 100)} aria-valuemin={0} aria-valuemax={100}>
              <div className="gow-track-fill h-full min-w-2.5 transition-[width] duration-300 ease-out" style={{ width: `${progress * 100}%` }} />
            </div>
            <ol className="flex flex-col gap-2">
              {Array.from({ length: stageCount }, (_, index) => {
                const done = index < currentStage;
                const current = index === currentStage;
                return (
                  <li key={index} className="flex items-center gap-2.5">
                    <CheckDot done={done} current={current} />
                    <span className={`truncate text-sm ${current ? 'font-semibold text-ink' : done ? 'text-ink-soft' : 'text-ink-muted'}`}>{t(`wizStage${index}`)}</span>
                  </li>
                );
              })}
            </ol>
            {currentStage >= 6 && <p className="text-[13px] text-warn">{t('wizOptimizationNotice')}</p>}
          </Body>
        )}

        {step === 5 && (
          <Body>
            <div className="flex flex-1 flex-col items-center justify-center gap-3.5">
              <span className="flex h-16 w-16 items-center justify-center rounded-full border border-gold-light bg-gold">
                <Check size={30} className="text-cream" strokeWidth={3} />
              </span>
              <p className="text-center text-[15px] text-ink-soft">{t('wizSuccessText')}</p>
              <p className="text-[13px] text-ink-muted">{f('wizTimeElapsed', duration(seconds))}</p>
            </div>
          </Body>
        )}

        {step === 6 && (
          <Body>
            <Lead>{t('wizErrorText')}</Lead>
            {errorHint && <p className="text-[13px] font-semibold text-ink">{errorHint}</p>}
            <pre className="max-h-[190px] overflow-auto whitespace-pre-wrap break-all rounded border border-bad bg-field p-2.5 font-mono text-xs text-ink">{errorText}</pre>
            <p className="text-[13px] text-ink-muted">{t('wizErrorHint')}</p>
          </Body>
        )}

        {step === 4 && confirmingCancel && (
          <p className="rounded-md border border-warn bg-field p-3 text-[13px] text-warn">{t('wizConfirmCancel')}</p>
        )}

        <div className="mt-1 flex items-center gap-2.5">
          {(step <= 3 || step === 6) && <SecondaryButton onClick={closeWizard}>{t('wizCancel')}</SecondaryButton>}
          {step === 6 && <SecondaryButton onClick={() => void api.openInstallLog()}>{t('wizViewLog')}</SecondaryButton>}
          <span className="flex-1" />
          {step >= 1 && step <= 3 && <SecondaryButton onClick={back}>{t('wizBack')}</SecondaryButton>}
          {step === 4 && !confirmingCancel && <SecondaryButton onClick={() => setConfirmingCancel(true)}>{t('wizCancelInstallation')}</SecondaryButton>}
          {step === 4 && confirmingCancel && (
            <>
              <SecondaryButton onClick={() => setConfirmingCancel(false)}>{t('wizKeepInstalling')}</SecondaryButton>
              <SecondaryButton danger onClick={() => void api.installCancel()}>{t('wizYesCancel')}</SecondaryButton>
            </>
          )}
          {step <= 2 && <PrimaryButton disabled={!canAdvance} onClick={next}>{t('wizNext')}</PrimaryButton>}
          {step === 3 && <PrimaryButton onClick={next}>{t('wizInstall')}</PrimaryButton>}
          {step === 5 && <PrimaryButton onClick={next}>{t('wizOk')}</PrimaryButton>}
          {step === 6 && <PrimaryButton onClick={next}>{t('wizTryAgain')}</PrimaryButton>}
        </div>
      </div>
    </Modal>
  );
}
