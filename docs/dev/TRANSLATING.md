# Translating the launcher

The launcher ships in English and Brazilian Portuguese. This page is the checklist for a new language and the rules
that keep it from shipping half done. **Adding a language is a maintainer decision**: it needs a native-speaker review
(see [Review policy](#review-policy)), so open an issue first.

## What is checked today

`gow-launcher/src/lib/i18n.test.ts` (run with `npm test` from `gow-launcher/`) fails when:

- the language files do not have identical key sets, or the code uses a literal key that `en.json` does not have;
- a file in `src/i18n/` is not registered in the `tables` map of `src/lib/i18n.ts`;
- a translation does not use the same `%1`, `%2`... placeholders as the English text (order may change, count may not);
- a translation is empty where English is not, has different leading or trailing whitespace, or is the key itself.

One failure lists every problem at once. Two pt-BR values that are still the English word (`integrated`, `dedicated`,
the graphics card kinds) are on a known list in the test; translating them removes the entry from the list, and the test
fails if the entry stays after the text changed.

## How to add a language

1. Copy `src/i18n/en.json` to `src/i18n/<code>.json` and translate the **values** only. Keep every `%N`, and literals
   such as `PLAY`, `ISO`, `FXAA`, key names and file names. `%1` is replaced first-occurrence-only
   (`format` in `src/lib/i18n.ts`), so a text that repeats one placeholder needs a rewrite.
2. Register it: `Language` in `src/lib/types.ts`, the `tables` map and `asLanguage` (map locale prefixes such as `es-MX`
   to `es`; default `en`) in `src/lib/i18n.ts`, and the options of the Language `Select` in `src/pages/OtherPage.tsx`
   (use the language's own name). `upper()` in `i18n.ts` needs the locale for upper-casing: Turkish has the dotted
   and dotless I.
3. In Rust, `Settings::migrate` (`gow-launcher/crates/gow-core/src/settings.rs`) forces the language to `pt-BR` or
   `en` from a boolean computed from the locale prefix: replace it with the supported list and update the bootstrap
   call in `src-tauri/src/lib.rs` and its tests.
4. Run `npm test` and `npx tsc --noEmit`, then do the [layout pass](#layout-checklist).
5. Add the language to `docs/LAUNCHER.md` (Other tab) and to the table below.

New keys added by features go in one block at the end of every language file, prefixed per feature, and never reorder
existing keys.

## Review policy

A language ships only with at least one native-speaker review recorded in the pull request. A machine-only draft may be
merged for testing but must not go into a release: list it under *Drafts* below until it is reviewed, and take it out of the
Language `Select` before the release if it is still listed. Credit translators in `docs/THIRD_PARTY.md` or
the README only with their consent.

| Language | Status |
|---|---|
| English | Source text |
| Brazilian Portuguese | Maintainer |

Drafts: none.

## Layout checklist

German and Russian texts run longer than English. Walk every tab and the wizard at the window's real size and look for:

- clipped or wrapped labels in fixed-width controls (the key slots in the Controls tab are 108 px wide);
- the descriptions of the setting rows pushing a row taller than its neighbours;
- buttons whose text no longer fits (PLAY, the dialogs, the wizard's navigation);
- scripts without glyphs in the UI font (the stack is `Segoe UI, system-ui`; CJK and Cyrillic rely on the OS fallback).

## The in-game menu stays English or Portuguese

The cheat menu inside the game (F2) only knows Portuguese (any `pt*` code) and English: the launcher passes
`PC_LANGUAGE` (see `launch.rs`) and `gow_cheats.cpp` picks between those two. A third launcher language shows the in-game
menu in English. That is a documented limit, not something a translation can fix.
