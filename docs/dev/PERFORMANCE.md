# Measuring performance

The user-facing advice is in [`../PERFORMANCE.md`](../PERFORMANCE.md). This page is the procedure and the counters, so
that two measurements can actually be compared.

## Where the time goes

| Subsystem | Thread | Notes |
|---|---|---|
| EE (recompiled game code) | main | Cheap per instruction, but it waits on the VU1 and the GS |
| VU1 | GS worker | **The usual bottleneck.** Native blocks where the microcode was captured, interpreter elsewhere |
| GS (VIF1 unpack, GIF, rasterization) | GS worker + GPU | GPU-bound at high internal resolution |
| Presentation | main | Scaling shader, FXAA, vsync |
| IOP | IOP thread | Interpreted code plus native IRX modules; matters for audio and disc load |

Two structural facts drive most numbers:

- **Capture coverage is the VU1 performance.** Every optimization pass converts more microcode to native blocks, so a
  build made from a fresh capture is slower than the same build after playing the same area again.
- **The optimization is per area.** A level the port has never seen has no native blocks for its microcode.

## Counters

| Switch | Output | Use |
|---|---|---|
| `GOW_PERF_DIAG=1` + `GOW_PERF_FRAME_PC=0x001837B8` | `[gow-perf] t=… ee_ms=… vu_ms=… gs_ms=… upload_ms=… guest_flip_hz=… host_hz=…` once per second | Split by subsystem; the launcher always sets these |
| `GOW_DELTA_DIAG=1` | `[gow-speed] speed=… game=… wall=…` | Game time vs real time (1.00 = real). **Check this first** |
| `GOW_VU1_STATS=1`, `GOW_VU1_BUDGET_DIAG=1`, `GOW_VU1N_DIAG=1` | Native block hits/misses, budget cuts | Is the VU1 running native code? |
| `GOW_GS_HW_STATS=1`, `GOW_GS_HW_PROFILE=1` | GS backend counters and per-pass timing | GPU-bound or CPU-bound? |
| `GOW_IO_HIST=<s>`, `GOW_IOP_IO_HIST=<s>` | Hardware registers still touched by game code | Finds work that could become native |
| `GOW_SAMPLING=1` | Periodic samples of the running threads | Feed `ps2-dev sampling` / `sampling-win` for per-function attribution |
| `GOW_PERF_DIAG` + `ps2-dev perf-summary LOG --game` | Windows of 5 s in the loaded game only | Removes menus, loading and transitions from the average |

With `--game`, `ps2-dev perf-summary` keeps only the windows enclosed by `[gow-pad2:state] state=11 pending=0 levelReady=1`
reports, so menu flips do not pollute the average. Without `--game` it selects windows by time, which is useful for
loading.

## The procedure

1. **Build configuration**: Release, logs off, no LTCG. That is what `install.cmd`/`install.sh` produce; a `-FastDev`
   build is for iterating only.
2. **Environment**: exactly what the launcher gives the game (see [`ENVIRONMENT.md`](ENVIRONMENT.md#set-by-the-launcher)).
   `PS2X_GS_WORKER=1` in particular changes everything: without it the GS runs inline and audio and frame rate collapse.
   `cargo run -p gow-cli -- env` from `gow-launcher/` prints the environment the saved settings would produce.
3. **Scene**: the same save, the same input. A pad script (`GOW_PAD_SCRIPT="5:start,12:x,…"`) or `GOW_PAD_TEST=1`
   makes runs repeatable; a memory card copy makes them safe (`GOW_MC0`). Do not save over the card you use to play.
4. **Warm-up**: skip at least the loading and the first seconds after the level appears.
5. **Windows**: at least 10–15 s of steady state, and report the spread (min/max), not a single second.
6. **Compare like with like**: same scene, same settings, same build options, one change at a time, previous commit
   rebuilt with the same configuration as the baseline.
7. **Sanity**: `speed=1.00`, no other build running, laptop on the performance profile, game window focused.

## Reducing a regression

- If `vu_ms` grew: the VU1 path changed (native block coverage, interpreter, or the capture set).
- If `gs_ms`/`upload_ms` grew: the GS backend or the packet path changed.
- If `ee_ms` grew: a hot override or a syscall got heavier (`getenv` in a hot loop is the classic case; read the
  variables once into statics).
- If the frame time is fine but the window is slow: presentation (scaling, vsync, FPS mode).

Profiling per function: `GOW_SAMPLING=1` plus `ps2-dev sampling` (Linux, with `nm`) or `sampling-win` (Windows, with
the linker map: configure with `PS2X_MAP=ON`). The sampler executable itself is built from
`native/tools/profiling/sample.cpp`.

## Benchmarks without the game

Two tools reproduce parts of the frame deterministically:

- `native/tools/render/replay_vif_chain.cpp`: repeats a captured VIF1 chain. With `PS2X_GS_DISCARD_DRAWS=1` and
  `GOW_REPLAY_TIMES=N` it measures the VIF1/VU1 path without rasterization.
- `native/tools/render/replay_gs.cpp`: replays a GS command capture against the CPU/GL backends and compares outputs.
  It is a correctness oracle, not a frame-rate benchmark (it serializes the GS).

## Landmines

- Measuring with logs on (`PS2X_ENABLE_RUNTIME_LOGS=ON`) or with a `-FastDev` build: numbers are not comparable.
- Measuring while another build, another game or a video call is running.
- Trusting the window FPS: the game is capped at 60 Hz while the window can run at 120/144.
- Comparing scenes with different enemy counts: VU1 cost follows what is on screen.
- Forgetting the power profile: on laptops it can halve the frame rate by itself.
