# Vendored: Rabbitizer

MIPS instruction decoder (C library plus its Rust bindings), used by `ps2-r5900` for the disassembly text that goes into the
generated comments.

- Upstream: https://github.com/Decompollaborate/rabbitizer
- Version: 1.14.3 (tag `1.14.3`, commit `d5804fa488474830a47fbf9f3693048ec1e45589`)
- License: MIT (`LICENSE`)

Only what the Rust build needs is vendored: `rust/`, `src/`, `include/`, `tables/`, `Cargo.toml`,
`Cargo.lock`, `README.md` and `LICENSE` — unmodified copies of upstream.

To update: replace those paths with the ones from the new upstream tag and update the version, tag and commit above. The
disassembly text is compared byte for byte by `ps2-parity`, so run it after a bump.
