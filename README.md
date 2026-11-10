# Wire Antenna Analyser

A Linux desktop program for modelling wire antennas: impedance, SWR, gain, front-to-back,
radiation patterns (2D and 3D), frequency sweeps and an optimizer. It is written in Rust with
egui, and the heavy calculations run on every processor core.

It was built as an independent Linux take on the classic MMANA-GAL basic workflow. It is **not**
affiliated with or endorsed by the MMANA-GAL authors (JE3HHT, DL1PBD, DL2KQ), and contains none of
their code or files. MMANA-GAL basic is freeware for private use from <http://www.gal-ana.de/basicmm/>.

## Features

- **Three engines**
  - built-in multi-threaded MININEC-style solver (the method MMANA uses)
  - built-in multi-threaded **wiremom** solver ([wiremom](https://github.com/tanganyika/wiremom)): MPIE with triangle currents and Galerkin testing, perfect or Sommerfeld real ground, all in-process; sweeps share Sommerfeld tables
  - bundled public-domain NEC-2 (`nec2c`) with Sommerfeld-Norton real ground; sweeps run many NEC-2 processes in parallel
- **Files:** open and save MMANA `.maa` (English and Russian headings), NEC `.nec` (including 4nec2 `SY` symbols, helices, arcs, GM/GR/GX), and EZNEC `.ez`
- **Tabs:** Geometry, 3D view (click a wire to zoom in, scale bar), Calculate, Far field plots (movable gain marker), 3D pattern (ARRL / dB / linear), Frequency sweep, Optimizer
- **Modelling:** wire materials, lumped loads (R+jX, L/C/Q traps, series/parallel RLC), wire insulation (velocity factor), free / perfect / real ground, F/B rear range
- **Ground systems (wiremom engine):** wires may run below z = 0 over real ground — ground rods, buried radials and counterpoises are modelled in full, including the ground loss
- **Convenience:** the last folder used for open/save is remembered

## Install on Linux

You need Rust (<https://rustup.rs>) and a C compiler (`sudo apt install build-essential` on
Debian/Ubuntu/Zorin). Cargo fetches the wiremom engine from
<https://github.com/tanganyika/wiremom> at the commit pinned in `Cargo.lock`; while that
repository is private, your git needs access to it (for example `gh auth setup-git`). Then:

```sh
./install.sh        # builds if needed, installs to ~/.local (menu entry, icon, file types)
./uninstall.sh      # removes it again
```

Or just build and run:

```sh
cargo build --release
./target/release/wireant [antenna.maa|.nec|.ez]
```

Command-line options: `--tab <name>` opens a tab, and `--screenshot out.png` saves a picture of the
window and exits (used for automated checks).

## Project layout

| Path | What |
|---|---|
| `src/mom.rs` | built-in solver (segmentation, matrix fill, LU, far field, sweeps) |
| `src/nec.rs` | NEC-2 engine bridge (deck writer, report parser) |
| `src/wm.rs` | wiremom engine bridge (antenna → wiremom model, results → shared result types); the engine itself is the [wiremom](https://github.com/tanganyika/wiremom) crate |
| `nec2c/`, `build.rs` | bundled nec2c 1.3.1 source (public domain) and its build |
| `src/maa.rs`, `src/nec_file.rs`, `src/ez.rs` | file formats |
| `src/optimize.rs` | optimizer |
| `src/app.rs`, `src/plots.rs` | user interface and plots |
| `MECHANICS.md`, `mechanics.json` | where each formula and value came from |
| `tools/cc.sh` | C compiler/linker helper (falls back to `zig cc` when no `cc` exists) |

## Working on the wiremom engine at the same time

To build against a local wiremom checkout (`../wiremom`) instead of the pinned GitHub
commit, create `.cargo/config.toml` (git-ignored) containing:

```toml
[patch."https://github.com/tanganyika/wiremom"]
wiremom = { path = "../wiremom" }
```

While that is in place Cargo rewrites `Cargo.lock` without the pinned commit, so don't
commit `Cargo.lock` then. When the engine change is done: push wiremom, delete the file,
and run `cargo update -p wiremom` to pin the new commit.

`cargo test --release` runs the tests (solver accuracy, file round trips, NEC-2 agreement).

## Credits

- NEC-2: Lawrence Livermore National Laboratory (public domain); C translation `nec2c` by Neoklis Kyriazis (5B4AZ), bundled in `nec2c/` with its own README and COPYING.
- The wiremom engine: [wiremom](https://github.com/tanganyika/wiremom), licensed MIT OR Apache-2.0.
- The MININEC method: J. C. Logan and J. W. Rockway, Naval Ocean Systems Center.
- The EZNEC file layout follows the notes collected by the open-source `ezparse` project (S. Falco and H. McGavran); no code from it is used.

## License

Copyright (C) 2026 tanganyika

This program is free software: you can redistribute it and/or modify it under the terms of the
GNU General Public License as published by the Free Software Foundation, either version 3 of the
License, or (at your option) any later version. See [LICENSE](LICENSE).

This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without
even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.

The bundled `nec2c` source keeps its own terms (public domain, per its README), and the wiremom
engine is MIT OR Apache-2.0; both may be distributed as part of this GPL program.
