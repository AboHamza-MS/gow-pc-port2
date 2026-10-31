# Custom textures

The port can draw **PCSX2 texture packs**: a pack made for PCSX2 (`textures/SCUS-97399/replacements`) works here
unchanged. This page explains how to install a pack, how to make one, and what is deliberately not supported.

## Installing a pack

1. **Textures → Custom textures** → on.
2. Either:
   - **Texture pack → Download and install** downloads the community pack (about 3.7 GB), extracts the pack's
     `replacements/*.png` next to `<installation>/textures/replacements`, swaps it in once every file is written and
     then deletes the ZIP. It needs about twice the pack size free on that drive and refuses to start otherwise. The card
     disappears once a pack is installed. It takes several minutes: one bar shows the download and then the extraction,
     and **while it runs the launcher holds still**: the other sections, PLAY and the options of this section are off,
     and closing the window asks first, because closing loses the download.
   - **Textures folder → Open**, then copy the pack's `.png` / `.dds` files into `replacements` yourself (subfolders
     are fine: only the file name counts).
3. Start the game. A change applies the next time the game opens.

> **The download replaces what is already in `replacements`**: once the new pack is complete, the folder is swapped
> for it, so a pack you assembled by hand is lost. Back it up first. If the install fails or is interrupted, the folder
> you had is left untouched.

The log (`logs/last.log`) says what was found: `[gow-texrepl] <folder>: N replacements`.

## Making a pack

Pack files are named after hashes of the data the game's graphics synthesizer holds, exactly as in PCSX2:

```
<texture>-<clut>-<bits>.png          # paletted texture (most of God of War's)
<texture>-<bits>.png                 # direct color
<texture>-<clut>-r<w>x<h>-<bits>.png  # CLAMP region mode reads only part of it
```

| Part | Meaning |
|---|---|
| `texture` | XXH3-64 of the texture's blocks as the GS holds them (for 24- and 16-bit formats, of the expanded texels; the formats are stored in the high bits, as are textures smaller than a GS block) |
| `clut` | XXH3-64 of the 16 / 256 palette entries (RGBA words, after TEXA) |
| `r<w>x<h>` | Present only when CLAMP's region modes read part of the texture |
| `bits` | `PSM \| TW << 6 \| TH << 10`, plus TEXA's TA0/AEM/TA1 in bits 15/23/24 for 24- and 16-bit formats |

- Hashes are hexadecimal **without leading zeros**.
- If two files have the same name, the first one found is used, as in PCSX2.
- Alpha is the GS's: `0x80` is fully opaque. A PNG without an alpha channel is treated as `0x80`.

This port has **no texture dump**: use PCSX2's dump to get the names, then edit the images.

## What is not replaced

- Textures read through a **CLAMP region** (`-r<w>x<h>` names).
- Textures the game draws into and then samples back from a render target (`rsrc` / `src` paths); PCSX2 does not
  replace them either.
- Any texture the game has not yet drawn at least once: the loader is demand-driven.

## Memory and smoothness

- Replacements are decoded on a background thread (PNG with stb_image, DDS/BC with bcdec).
- By default, decoded images are kept on the CPU side up to 768 MB and on the GPU up to 1536 MB; the least recently
  drawn are dropped first and decoded again if drawn again.
- At most 48 MB are uploaded to the GPU per 16 ms, so a view full of new textures does not stall a frame.
- **Load textures asynchronously** (on by default) matches PCSX2's "Load Textures Asynchronously": the game's own
  texture is drawn until the replacement is ready. Turning it off makes the game wait for each new texture (up to 5 s),
  but the replacement shows from the first frame.

## Under the hood

The loader is `RustPortPS2/native/ps2xRuntime/src/lib/gs/gpu/gs_texture_replace.cpp`; the upload happens in
`GSHwBackend::replacementFor`. A texture's name is computed when a draw uses it (once per texture/palette/TEXA, and
again after its memory is rewritten), looked up in the folder's index, decoded, and uploaded with mip levels on first
draw. The shader still normalizes coordinates by the original texture size, so a replacement works at any resolution.

Advanced switches:

| Variable | Effect |
|---|---|
| `GOW_TEXTURE_REPLACE=<folder>` | Indexes that folder (the launcher sets `<installation>/textures/replacements`) |
| `GOW_TEXTURE_REPLACE_SYNC=1` | Waits for each file instead of drawing the game's texture meanwhile |
| `GOW_TEXTURE_REPLACE_CPU_MB`, `GOW_TEXTURE_REPLACE_GPU_MB` | Memory budgets (see above) |
| `GOW_TEXTURE_REPLACE_UPLOAD_MB` | MB uploaded per 16 ms (default 48) |
| `GOW_TEXTURE_REPLACE_DIAG=1` | Logs the computed name and `replacement` / `no file` for the first 400 textures, plus each upload. Use it to compare names with a PCSX2 pack |

The naming and hashing follow PCSX2 (`GSTextureReplacements.cpp`); the structure of the loader follows
ps2-port-engine's `texture_pack`. Licenses and credits: [`THIRD_PARTY.md`](THIRD_PARTY.md).
