# native: the runtime that executes the recompiled game

The C++ half of the port. The Rust recompiler (`../crates`) turns `SCUS_973.99` into C++; this folder holds what that
C++ runs on:

- `ps2xRuntime/`: the EE context and memory, kernel and syscall services, GS (frontend, native OpenGL renderer,
  threaded backend), VU0/VU1, DMA, SIF, audio, the pad and the window.
- `ps2xIOP/`: the R3000A IOP, the virtual IOP kernel and the native (ahead-of-time) IRX modules.
- `ps2xTest/`: runtime tests (`-DPS2X_BUILD_TEST=ON`).
- `tools/`: the VU1 C++ generator, the replay/benchmark tools and the sampler.
- `tests/`: the C++ tests and their CMake fragments.

It is based on [PS2Recomp](https://github.com/ran-j/PS2Recomp) by ran-j and on
[Waffle565/God-Of-War-Native-Port](https://github.com/Waffle565/God-Of-War-Native-Port), and keeps the license of the
latter in [`LICENSE`](LICENSE). Mark changes to that upstream code with a `// GOW-Port:` comment.

`install.ps1` / `install.sh` build it (`cmake -S RustPortPS2/native`). They first run `ps2_recomp` and copy the
generated sources into `ps2xRuntime/src/runner/` and `ps2xRuntime/include/` (both ignored by git). To build and test
by hand, see [`../../docs/dev/BUILD.md`](../../docs/dev/BUILD.md).
