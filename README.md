# Wire Antenna Analyser

A Linux desktop program for modelling wire antennas: impedance, SWR, gain, front-to-back,
radiation patterns (2D and 3D), frequency sweeps and an optimizer. It is written in Rust with
egui, and the heavy calculations run on every processor core.

It was built as an independent Linux take on the classic MMANA-GAL basic workflow. It is **not**
affiliated with or endorsed by the MMANA-GAL authors (JE3HHT, DL1PBD, DL2KQ), and contains none of
their code or files. MMANA-GAL basic is freeware for private use from <http://www.gal-ana.de/basicmm/>.

## Features

- **Three engines** (choose under Calculate → Engine; see [Engines compared](#engines-compared))
  - **wiremom** (default): built-in, multi-threaded, in-process
    ([wiremom](https://github.com/tanganyika/wiremom)). It uses MPIE with
    triangle currents and Galerkin testing, perfect or Sommerfeld real ground,
    wires in the ground, transmission lines and plates. Sweeps share Sommerfeld
    tables.
  - **MININEC-style** solver: the method MMANA uses. Real ground only affects the
    far field.
  - **NEC-2**: bundled public-domain `nec2c` with Sommerfeld-Norton real ground.
    Sweeps run many NEC-2 processes in parallel.
- **Files:** open and save MMANA `.maa` (English and Russian headings), NEC `.nec`
  (including 4nec2 `SY` symbols, helices, arcs, GM/GR/GX, `TL` lines and `SM`/`SC`
  plates), and EZNEC `.ez`.
- **Tabs:**
  - Geometry;
  - View:
    - click a wire to zoom in;
    - right-click a wire to edit it (length with the start, centre or end kept
      fixed, end points, radius, segments, insulation; joined wires follow);
    - scroll bars and right-drag move the view;
    - scale bar;
  - Calculate;
  - Far field plots (movable gain marker);
  - 3D pattern (ARRL / dB / linear);
  - Frequency sweep;
  - Optimizer.
- **Modelling:** wire materials, lumped loads (R+jX, L/C/Q traps, series/parallel
  RLC), wire insulation (velocity factor), free / perfect / real ground, F/B rear
  range.
- **Transmission lines (wiremom and NEC-2 engines):** feed lines, phasing lines and
  stubs between any two places on wires. Each has an impedance, length, velocity
  factor, loss and reversal.
- **Plates (wiremom engine):** flat conducting plates (reflectors, ground screens,
  car-body panels) meshed into triangles. A wire ending on a plate connects to it,
  so a monopole can be fed against a plate.
- **Ground systems (wiremom engine):** wires may run below z = 0 over real ground.
  Ground rods, buried radials and counterpoises are modelled in full, including
  the ground loss.
- **Convenience:** the last folder used for open/save is remembered.

## Engines compared

| | wiremom (default) | MININEC | NEC-2 |
|---|---|---|---|
| Method | MPIE, triangle currents, Galerkin | MININEC pulses | NEC-2 EFIE (nec2c) |
| Runs | in-process, all cores | in-process, all cores | one process per calculation |
| Real ground in the currents and impedance | Sommerfeld | no (perfect ground; real ground only in the pattern) | Sommerfeld-Norton |
| Wires in or through the ground | yes | no | no |
| Wires stopping on real ground | stand-in over perfect ground (warned) | same | not reliable (warned) |
| Transmission lines | yes, with loss | no | yes, lossless |
| Plates | yes | no | no (NEC-2 patches are for closed surfaces) |
| Steel wire (permeability) | yes | yes | approximated (σ/μr) |

On the six built-in examples the three engines agree within 0.07 dB in gain. The
impedance spread is a few percent, except the Inverted V's reactance, where MININEC
differs from the other two.

## Known limitations

These are the weaker spots, with how far they have been checked.

- **Wire-to-plate junctions (monopole fed against a plate):** nothing independent
  confirms the exact impedance.
  - The plate model converges smoothly as the mesh is refined: 39.9 Ω for a λ/4
    monopole on a 1 λ square plate, in line with published values for that size
    of ground plane.
  - Energy is conserved.
  - An equivalent wire-grid model differs by up to about 8%, approaching the
    plate result as the grid gets finer. (Wire grids themselves are known to be
    unreliable at a feed junction.)
- **Wires in the ground:** no other program available here can model them.
  nec2c can't: it gives a meaningless 259 − j232 Ω for a vertical connected to
  average ground. The engine is checked against exact limits and a classic
  ground-rod formula (Dwight, within 0.7%), not against measurements or NEC-4.
- **Wires that stop on the surface of real ground** with nothing below it (no rod
  or radials) use a perfect-ground stand-in for the currents. A point contact into
  lossy earth can't be modelled. The warning suggests adding a rod or radials.
- **Speed with buried wires:** the air-to-ground couplings are computed directly,
  not tabulated. A vertical with 8 buried radials takes about 1.4 s per frequency,
  so sweeps of such models are slow.
- **Plates:** flat parallelograms only, above the ground (not in it). Large
  plates grow the problem quickly; a 1 m plate at 144 MHz is fine, but meshes of
  thousands of triangles take seconds.
- **NEC-2 engine:** plates are refused. Line loss is ignored, and a velocity
  factor becomes the equivalent electrical length. With automatic segmentation,
  NEC-2 gets uniform segments, because it is inaccurate next to the very short
  segments of the MMANA-style taper.
- **Files:** transmission lines in EZNEC files are skipped on import. `.maa` files
  can't hold transmission lines or plates; use `.nec` to keep them.
- **Wire editor:** changes apply immediately. Press Calculate again for new
  results; the old results stay on screen until you do.

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
