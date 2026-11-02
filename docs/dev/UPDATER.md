# Launcher self-update

How the launcher finds, downloads and installs a newer version of itself, and what the maintainer does to make a release
updatable. What the player sees is in [`../LAUNCHER.md`](../LAUNCHER.md#updating-the-launcher).

## How it works

```
GitHub API: releases of the repository
   |  newest published, stable `launcher-vX.Y.Z` release (semver; drafts, pre-releases and other tags ignored)
   v
newer than the running launcher? --no--> nothing is shown
   |yes
   v
this copy can update itself?  (installed, release has latest.json, public key is real)
   |yes                                   |no (portable zip, .deb, unsigned release, placeholder key)
   v                                      v
latest.json of that release is handed   notice with "Open release page"
to tauri-plugin-updater as its endpoint
   |  signature check, download, install
   v
Windows: the NSIS installer runs (passive) and starts the new launcher.  Linux AppImage: file replaced, launcher restarts.
```

- **Decision logic** (which release, install kind, mode, blockers, schedule): `gow-launcher/crates/gow-core/src/update.rs`, with
  tests.
- **Shell** (plugin, events, commands `update_state`, `update_install`, `update_open_release`): `src-tauri/src/updater.rs`.
- **Interface**: `UpdateNotice` in `src/components/Shell.tsx` (title bar), state in `src/state/app.tsx`, the *Automatic
  updates* switch in `src/pages/OtherPage.tsx`.
- **Why the plugin and the GitHub API together.** The plugin does what must not be hand-written (minisign signature check
  of the downloaded installer, the NSIS and AppImage install steps, restart). Its endpoint, though, would normally be the
  fixed `releases/latest/download/latest.json`, which follows GitHub's single "latest release" of the whole repository and
  has no notion of the `launcher-v*` tag family. So the launcher lists the releases itself, picks the newest launcher
  release by semver, and gives *that* release's `latest.json` to the plugin at run time. The same listing also gives the
  release page that a portable copy opens.

### When it runs

- First check about 8 seconds after the launcher opens, then at most every 6 hours while it stays open (30 minutes after a
  failed check). A failure (offline, rate limit, a release without the manifest) is written to `logs/update.log` in the data
  folder and shown to nobody.
- With **Automatic updates** on, an update found is installed as soon as nothing is running; with it off, only the notice
  appears. An automatic attempt that fails is not repeated by itself: the notice shows the error and a **Retry** button.
- **Never while** the game, an installation (wizard), an optimization or a texture-pack install is running. The backend
  refuses (`busy:<reason>`) and the interface shows the button disabled with the reason. If one of them starts while the file
  is downloading, the installer waits, already downloaded, for the next safe moment.
- `GOW_LAUNCHER_NO_UPDATE_CHECK=1` turns the checks off (tests, development).

### Which copies update themselves

| Copy | Behavior |
|---|---|
| Windows, installed with the setup program (`uninstall.exe` beside the program) | In place |
| Linux AppImage (`APPIMAGE` is set) | In place (not exercised on a real Linux machine yet) |
| Windows portable zip, or a build folder | Notice with **Open release page**; the text says the zip cannot update itself |
| Linux `.deb` or a build folder | Notice with **Open release page** (a package install needs the package manager) |
| Any copy, but the release has no `latest.json` or the public key in the config is the placeholder | Notice with **Open release page** |

The `.deb` is deliberately not updated in place: the plugin could run `dpkg` through `pkexec` or `sudo`, but a launcher that
asks for a password in the middle of an update is not something to ship unverified.

## One-time setup (maintainer)

The updater only installs files signed with a key whose public half is in the launcher. **State since v0.2.5**: the key pair
exists (steps 1 and 2 are done, the public key is in `src-tauri/tauri.conf.json`), and step 3, the two GitHub secrets, is the
maintainer's. A launcher older than v0.2.5 was built with the placeholder key (`REPLACE_WITH_THE_UPDATER_PUBLIC_KEY`) and can
never verify a signed release: its owners install v0.2.5 by hand once, from the notice's "Open release page" button, and every
release after that updates in place. Until the secrets exist a release is built without signed files and the notice's button
opens the release page.

1. **Generate the key pair** on your machine, from `gow-launcher/`:

   ```bash
   npx tauri signer generate -w ~/.tauri/gow-launcher.key
   ```

   It asks for a password (use one) and writes two files: the private key (`gow-launcher.key`) and the public key
   (`gow-launcher.key.pub`). **Never commit, paste in chat or print the private key or its password.** Keep a backup in a
   password manager: if the key is lost or replaced, launchers already installed cannot verify the next release and their
   owners have to install it by hand once.
2. **Put the public key in the config.** Copy the *contents* of `gow-launcher.key.pub` (one line of base64) into
   `plugins.updater.pubkey` of `gow-launcher/src-tauri/tauri.conf.json`, replacing the placeholder, and commit. The public
   key is not a secret. A test (`cargo test -p gow-launcher`) checks that the value is either the placeholder or a valid
   minisign key.
3. **Add the secrets** in GitHub, *Settings, Secrets and variables, Actions, New repository secret*:
   - `TAURI_SIGNING_PRIVATE_KEY`: the contents of `gow-launcher.key` (the file's text, not its path).
   - `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`: the password you chose.

   The workflow (`.github/workflows/gow-launcher.yml`) uses them only in the `release` job. With the first secret missing the
   release is built as before, with a warning and no signed files; with it present but the placeholder still in the config
   the job fails on purpose.
4. **Optional hardening.** After the first signed release, decode one `.sig` (`base64 -d`) and check that its trusted comment
   has `version:<x.y.z>`. If it does, add `"requireSignedVersion": true` to `plugins.updater` so that a response which pairs
   a newer version number with an older signed installer is refused. (The plugin already checks the version when the
   signature carries it; this flag also rejects signatures that carry none.)

## Cutting an updatable release

1. Bump the version in **all three** places, plus the lock files, and commit: `version` in the workspace `Cargo.toml`
   (`[workspace.package]`), in `src-tauri/tauri.conf.json` and in `package.json`. The release job refuses a tag that disagrees
   with them: a launcher that thinks it is older than its own release would offer the update again after every install.
2. Tag `launcher-vX.Y.Z` and push the tag. The workflow builds the NSIS installer, the `.deb` and the AppImage, signs the
   NSIS installer and the AppImage, attaches everything to a **draft** release and uploads `latest.json`.
3. Before publishing the draft, check its assets:
   - `…-setup.exe` and `…-setup.exe.sig`, `….AppImage` and `….AppImage.sig`, the portable zip, `SHA256SUMS-*.txt`;
   - `latest.json` with **both** `windows-x86_64` and `linux-x86_64` entries, each with a `signature` and a `url`. The two
     jobs merge into one file; if one platform is missing (the jobs ran at the same moment), re-run the job that is missing.
   - the `url` values point to `releases/download/launcher-vX.Y.Z/…` (open one in a browser after publishing).
4. Publish the release. A draft or a pre-release is invisible to the launcher; only a published, non-pre-release
   `launcher-vX.Y.Z` is offered.

The first updatable release cannot be installed by the in-app update, since the launchers already out do not have it: people
install that one by hand. To try the whole path, install a signed build **A**, then publish a signed **A+1** and open **A**.

## Things to know

- **Size.** The updater installs the same NSIS setup program as a fresh install, and that one bundles Microsoft's offline
  WebView2 installer (about 214 MiB). Every update therefore downloads that much. The progress bar shows it. A smaller
  update-only installer would need a second NSIS build without WebView2; it is not done.
- **SmartScreen and the updater signature are different things.** The signature above protects the update from tampering.
  The Windows program is still unsigned for SmartScreen, which has no effect on an update that the launcher itself started.
- **The portable zip** never updates itself and is not touched by the installer.
- **Seeing it without a release.** `npm run dev` and open `http://localhost:1420/?update=automatic` (or `portable`, `manual`)
  to see the notice with the browser mock.
- **Log.** `logs/update.log` in the data folder: every failed check, the manifest it used and the install steps.
- **Not exercised here.** The plugin's download and install against a real signed release, the AppImage replacement and the
  restart on Linux, and the single-instance plugin when the launcher restarts itself. Test them with the first signed pair
  of releases (A and A+1 above) before telling anyone to turn Automatic updates on.
