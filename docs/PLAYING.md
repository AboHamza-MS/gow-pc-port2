# Playing

Everything you can press, change and carry between sessions. Installation is in [`INSTALL.md`](INSTALL.md); every
launcher option is in [`LAUNCHER.md`](LAUNCHER.md).

## Keyboard and mouse

The port ships with a keyboard and mouse layout that you can **remap** key by key in **Controls**. Each action accepts
two keys (or mouse buttons). The default layout:

| Action | Default | Action | Default |
|---|---|---|---|
| Move | **W A S D** | Evade (right stick) | **Ctrl** + **W A S D** |
| Light attack (□) | **Left click** | Heavy attack (△) | **Right click** |
| Jump (✕) | **Space** | Grab (○) | **F** |
| Block / parry (L1) | **Q** | Action (R1) | **E** |
| Cast magic (L2) | **Left Shift** | Interact (R2) | **Ctrl** (while standing still) |
| Rage of the Gods | **V** | Blade of Artemis (L1 + R1) | **Middle click** |
| Poseidon's Rage | **1** | Medusa's Gaze | **2** |
| Zeus' Fury | **3** | Army of Hades | **4** |
| D-pad | **Arrow keys** | Right stick (camera) | **I J K L** |
| Power menu (Start) | **Enter** | Select | **Tab** |

Notes:

- Ctrl deliberately does two jobs: **R2** when you are standing still and **evade** when you move. The runtime only
  counts the key as a button while no movement key is held.
- Keys are bound by **physical position**, so the layout works on any keyboard layout (including ABNT2 and Turkish).
- Gamepad buttons can be swapped one by one (✕ ○ □ △); the presets are **Classic (PS2)**, the default, and **Modern
  (DMC5 style)**. **Custom (every button)** lets you choose what each of the 16 buttons presses (see [Gamepad](#gamepad)).

## In-game shortcuts

| Key | What it does |
|---|---|
| **F11** | Toggle fullscreen |
| **F2** | Open / close the cheat menu (also on a gamepad button or combo, see below) |
| **F1** | Developer panel (frame timings, renderer state). Not needed to play |
| **Esc** | Does nothing by default. Turn on **ESC closes the game** in **Controls** to make it quit |

## Cheat menu (F2)

**F2** opens an in-game menu; the game keeps running behind it.

- **↑ ↓** (or W / S) choose, **Enter / Space / ← →** toggle, **F2 / Esc / Backspace** close.
- With the mouse: hover chooses, left click toggles.
- With a gamepad: d-pad chooses, ✕ toggles, ○ closes. The launcher can also capture **one button or two buttons held
  together** to open it (Gamepad capture is Windows-only); F2 always works.
- While the menu is open the game receives no input, so you cannot move by accident.

The cheats (the same list appears under **Cheats** in the launcher, where you choose which ones start enabled):

| Cheat | Effect |
|---|---|
| God mode | Kratos takes no damage and his health stays full |
| One-hit kills | Enemies and bosses die in one hit (Kratos is never affected) |
| Infinite magic | The magic meter never drains |
| Infinite Rage of the Gods | The Rage meter is always full |
| Max red orbs | 60,000 orbs, the game's cap |
| All upgrades | Blades of Chaos level 5, magic and Blade of Artemis level 3 |
| All powers and weapons | Zeus' Fury, Medusa's Gaze, Poseidon's Rage, Army of Hades, Blade of Artemis, Poseidon's Trident |
| Max health and magic | Every Gorgon Eye and Phoenix Feather already applied |
| Unlock all extras | God Mode difficulty, costumes, videos and Challenge of the Gods |

Changes made inside the menu are **not** written back to the launcher: the list there is what starts enabled next time.
`unlock_extras` is the exception: once you save, the unlock stays on the memory card.

## Target lock (Alt)

A lock-on in the style of the modern God of War games, always available:

| Input | Effect |
|---|---|
| **Alt**, pressed and released alone | Locks the closest enemy (within 1500 units, favoring the center of the screen); press again to release |
| **Mouse left / right** (60 px) | Switches to the next enemy on that side |

- It only locks when Alt was pressed *alone* and for less than 0.6 s, so **Alt+Tab** never locks anything.
- Attacks go to the locked enemy; when it dies or gets farther than 3000 units the lock moves to the closest one.
- A small red diamond marks the target.
- The camera cannot be locked (the game's camera is scripted), so this is a targeting aid, not a modern third-person
  camera.

## Agile combat

On by default (**Controls → Agile combat**). It changes only the *ends* of the game's own movements:

- an attack from a dodge comes out earlier (20 % of the roll instead of 50 %);
- an attack press the game refused (for example on the frame you release block) is retried for a moment;
- cancelling an attack with block or evade is **already in the original game** and is untouched.

If you prefer the original timing, turn the switch off.

## Gamepad

- Any XInput (Windows) or evdev (Linux) pad works; DualShock 4, DualSense, Xbox and 8BitDo controllers were tested.
- The PS2 layout is the default; the modern layout and the four face-button swaps are in **Controls**.
- **Custom (every button)** in **Controls** gives each of A, B, X, Y, LB, RB, LT, RT, Back, Start, both stick presses and the
  four d-pad directions its own PS2 button (or Nothing). It starts from the layout you were using. Two buttons may press the
  same PS2 button; Start has to stay on some button. The sticks and the guide button are not remappable.
- **Vibration works on Windows only**: the Linux build has no rumble.
- Only port 1 is connected: the game's second controller slot does nothing.

## Saves and the memory card

- Saves are stored in a **PCSX2-compatible card image, `Mcd001.ps2`**, next to the game executable (`game/` inside your
  installation). It is an 8 MB card with ECC, the same format PCSX2 uses.
- **You can copy your PCSX2 card there** (any `Mcd001.ps2` / `Mcd002.ps2`) and the game will list those saves. The
  reverse works too: PCSX2 opens the card this port writes.
- Override the path with `GOW_MC0=<file>`. `GOW_MC0=0` leaves slot 1 empty; `GOW_MC1=<file>` enables slot 2.
- Loading and saving were verified end to end (list with thumbnails, "Load Complete", overwrite, "Save Complete").
- The `mc0/` folder next to `SCUS_973.99` holds the few `mc0:` file operations the game performs outside the card
  image. It is created automatically.
- Creating a save on a **blank** card (formatting it) has not been verified: start from a PCSX2-formatted card.

## Custom textures

PCSX2 replacement packs work here. **Textures → Custom textures** enables them, and the launcher can download the
community pack for you. Details, including how a pack's file names are computed, are in [`TEXTURES.md`](TEXTURES.md).

## Achievements (RetroAchievements)

**Achievements** in the launcher signs in with your RetroAchievements account and loads the game's achievement set when
the game starts.

- The account password is **not** stored with the settings: it goes to the Windows Credential Manager. On Linux the
  password store is not implemented yet, so the sign-in cannot be saved there.
- The game is identified from your original ISO, or from an unmodified `SCUS_973.99`; translated or patched ISOs are not
  recognized.
- **Hardcore mode** is required for hardcore achievements and **disables every cheat** (the cheat list is not even sent
  to the game).
- In-game notifications can be switched off.

## Language

The launcher is in **English** and **Brazilian Portuguese** (*Other → Language*). The in-game cheat menu follows the
same choice.
