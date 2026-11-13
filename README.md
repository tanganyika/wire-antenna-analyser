# wireant

A desktop program for Linux, Windows and macOS for modelling wire antennas: impedance, SWR, gain, front-to-back,
radiation patterns (2D and 3D), frequency sweeps and an optimizer. It is written in Rust with
egui, and the heavy calculations run on every processor core.

It was built as an independent take on the classic MMANA-GAL basic workflow. It is **not**
affiliated with or endorsed by the MMANA-GAL authors (JE3HHT, DL1PBD, DL2KQ), and contains none of
their code or files. MMANA-GAL basic is freeware for private use from <http://www.gal-ana.de/basicmm/>.

## Features

- **Three engines, plus your own NEC** (choose under Calculate → Engine; see [Engines compared](#engines-compared))
  - **wiremom** (default): built-in, multi-threaded, in-process
    ([wiremom](https://github.com/tanganyika/wiremom)). It uses MPIE with
    triangle currents and Galerkin testing, perfect or Sommerfeld real ground,
    wires in the ground, transmission lines and plates. Sweeps share Sommerfeld
    tables.
  - **MININEC-style** solver: the method MMANA uses. Real ground only affects the
    far field.
  - **NEC-2**: bundled public-domain `nec2c` with Sommerfeld-Norton real ground.
    Sweeps run many NEC-2 processes in parallel.
  - **External NEC** (experimental): your own licensed copy of NEC-4 or NEC-5, or
    another NEC program, as EZNEC does; see [Using your own NEC-4 or NEC-5](#using-your-own-nec-4-or-nec-5).
- **Files:** open and save MMANA `.maa`, NEC `.nec`
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
    - **Draw wires** mode:
      - *New wire*: drag to draw a wire; starting on a wire end joins it there.
        Release opens the wire editor so the exact length can be typed in.
      - *Adjust*: drag a wire end to change the length along the wire, or with
        Shift to move the end freely. Wire ends joined there follow. Drag a wire
        to move it.
      - Wires are drawn in the plane facing you (or a fixed XY, XZ or YZ
        plane) through the point the drag starts from, or through the origin.
        Ends snap to other wire ends and to a chosen grid.
      - A live readout shows the length and coordinates. Ctrl+drag rotates the
        view, and Undo (Ctrl+Z) reverses drawing changes;
  - Calculate;
  - Far field plots (movable gain marker);
  - 3D pattern (ARRL / dB / linear);
  - Frequency sweep;
  - Optimizer: changes wire lengths, positions or ends towards low SWR, jX near 0,
    high gain or high F/B. Wire ends joined to other wires or to the ground stay
    joined; click **Use optimized antenna** to keep the result.
- **Efficiency and power budget:** each calculation shows how much of the input
  power is radiated and how much is lost in the wires, loads, lines and the ground
  (Calculate tab, report and MCP), on every engine.
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
- **Second ground (all engines):** a different ground beyond a straight
  boundary at any bearing, or outside a circle, optionally lower (a cliff or
  bank), such as the sea next to a portable station or a salt marsh. As NEC-2's
  GD card, it changes the ground reflection in the patterns; the currents and
  impedance see the ground under the antenna. wiremom and MININEC can share each
  ray's reflection between the grounds over its first Fresnel zone, which matters
  when the shore is close. NEC-2 (and the "reflection point" option) uses the
  ground where each ray reflects. The View tab draws the boundary in blue. Ground
  presets fill in typical εr and σ.
- **Tools menu:**
  - **Resonance finder:** sweeps a range on all cores (31 points by default),
    finds every reactance zero, and refines each with a few extra solves. The
    bandwidth edges are found by solving too, so they're accurate even when the
    band is narrower than the point spacing. Large models take a while: about
    one second per solve at 1,350 unknowns, so about a minute for a search. For each resonance it shows
    the type (series or parallel), R, SWR, the SWR bandwidth against the
    reference impedance, the bandwidth once matched to its own resistance, and Q
    (f·|dZ/df| / 2R). Plots of SWR and R/jX are included. "Use frequency" sets
    the working frequency; "Match…" opens the match calculator with that
    impedance.
  - **Match calculator:** for an impedance (typed in, from the last
    calculation, or from a resonance) and a target (default 50 Ω):
    - every L network (L and C, series or shunt first), with values, marked as
      low-pass or high-pass;
    - a gamma match: rod length and series capacitor, from element and rod
      diameters and spacing (the ARRL Antenna Book transmission-line model;
      approximate, so check by modelling the rod);
    - coax sections: a Regier series section of a chosen impedance (75, 93,
      35 Ω, …), or a length of feeder to a real-impedance point plus a
      quarter-wave transformer, as electrical degrees and metres at a velocity
      factor;
    - **Add to the antenna** puts the chosen solution into the model: L-network
      parts on a line from a small feed wire, the gamma rod, strap and
      capacitor as wires (with the rod length and capacitor tuned by the model
      itself), or the coax sections as lines.
  - **Antenna report (PDF):** an A4 report of the antenna (settings, a picture
    as on the View tab with its viewing direction, wire numbers and current
    colours, loads, lines, plates, comment), the Calculate tab's latest result
    with its efficiency and power budget, notes and results history, the horizontal and vertical patterns, and the
    resonance finder's and match calculator's current results with their plots.
    The save dialog suggests the antenna file's name with `.pdf` in its folder,
    or `untitled.pdf` in the last folder used. The PDF is written directly, with
    vector plots and standard PDF fonts, so nothing else needs installing.
    Once saved, the report opens in the default PDF viewer.
- **Help** (Help menu, or F1 for the tab you are on): help pages built into the program.
  They are also in [docs/help](docs/help/README.md).
- **Preferences** (File → Preferences): whether to start with a blank antenna or the
  last one opened, the engine used at start-up and the external
  NEC program. The last folder used for open/save is remembered too.
- **MCP server:** `wireant --mcp` lets AI assistants build, load, calculate,
  sweep and optimise antennas; see [Use from an AI assistant (MCP)](#use-from-an-ai-assistant-mcp).

## Use from an AI assistant (MCP)

`wireant --mcp` runs the program as a [Model Context Protocol](https://modelcontextprotocol.io)
server instead of opening a window. An MCP client such as Claude Code or Claude Desktop starts
it and talks to it over stdin/stdout (JSON-RPC, one message per line). The server keeps one
current antenna between calls.

Claude Code:

```bash
claude mcp add wireant -- /path/to/wireant --mcp
```

Other clients take the same command in their configuration, e.g. Claude Desktop's
`claude_desktop_config.json`:

```json
{ "mcpServers": { "wireant": { "command": "/path/to/wireant", "args": ["--mcp"] } } }
```

| Tool | What it does |
|---|---|
| `list_examples` | built-in example antennas and which engines are available |
| `new_antenna` | start from an example or an empty antenna |
| `open_file`, `save_file` | `.maa`, `.nec` and `.ez` files, as in the program |
| `get_antenna`, `set_antenna` | the whole antenna as JSON |
| `update_antenna` | change it with a JSON Merge Patch, e.g. `{"freq_mhz": 7.1, "ground": {"kind": "real"}}` |
| `set_wire`, `delete_wire` | change, add or remove one wire (sources and loads on a deleted wire go too) |
| `calculate` | impedance, SWR, gain and direction, F/B, efficiency and power budget (radiated, wires, loads, lines, ground); optional azimuth and elevation pattern cuts |
| `sweep` | R, X, SWR (and gain, F/B) over a frequency range |
| `optimize` | the optimizer: wire lengths, positions or ends against SWR, X, gain and F/B weights |
| `report` | the PDF antenna report, as Tools → Antenna report: calculates, finds resonances and suggests matches itself; saved next to the antenna's file unless given a `path` |

`calculate`, `sweep`, `optimize` and `report` take an optional `engine` (`wiremom`, `mininec`, `nec2`,
or `external` once it is set up under Preferences),
and `calculate` and `report` an optional `freq_mhz`, for that call only. The server starts with the
Preferences start-up engine; `WIREANT_ENGINE` overrides it, as for the program. In the JSON, wires are numbered from 0; units are metres, MHz and ohms.

## Engines compared

| | **wiremom** (default) | **NEC-2** (nec2c) | **MININEC** |
|---|---|---|---|
| **How it calculates** | Mixed-potential method: current shapes overlap between neighbouring segments and are tested the same way | Classic NEC-2 method: point-tested, with a three-term current shape on each segment | Pulse currents, the original MININEC approach used in MMANA |
| **Real ground in impedance and currents** | Yes (Sommerfeld) | Yes (Sommerfeld–Norton) | No: perfect ground for the currents, real ground only in the pattern |
| **Wires in or through the ground** (radials, ground rods) | Yes | No | No |
| **Wires ending on real ground** | Treated as if the ground were perfect (warned) | Not reliable (warned) | Same as wiremom |
| **Transmission lines** | Yes, with loss | Yes, lossless only | No |
| **Plates** | Yes, flat plates | No (NEC-2 patches are only for closed surfaces) | No |
| **Steel wire** (permeability) | Yes | Approximated (σ/μr) | Yes |
| **Second ground area** (sea, marsh, cliff) | Gradual change across the boundary, or the ground where each ray reflects | Only the ground where each ray reflects (GD card) | Gradual change or reflection point, like wiremom |
| **How it runs** | Inside the program, on all cores; sweeps reuse the slow ground calculation | A separate program run for each calculation, many at once | Inside the program, on all cores |

On the six built-in examples the three engines agree within 0.07 dB in gain. The
impedance spread is a few percent, except the Inverted V's reactance, where MININEC
differs from the other two.

In practice:

- **Free space or perfect ground:** all three agree closely.
- **Low antennas over real ground:** MININEC's impedance can be off. Its currents assume
  perfect ground, so a low dipole's feed resistance and resonance shift compared with
  wiremom and NEC-2.
- **Ground-mounted verticals, buried radials, ground rods:** only wiremom models these properly.
- **Feed lines, phasing lines and stubs:** wiremom includes line loss, NEC-2 treats lines
  as lossless, and MININEC can't model them.
- **Comparing with MMANA:** MININEC is closest to MMANA.
- **Comparing with EZNEC, 4nec2 or other NEC tools:** NEC-2 is the standard reference.

## Using your own NEC-4 or NEC-5

NEC-4 and NEC-5 are licensed from Lawrence Livermore National Laboratory and can't be
bundled, but if you have a copy the program can run it, as EZNEC Pro does. Under
**File → Preferences → External NEC program**:

- **Program:** the NEC executable.
- **Arguments:** how it takes its file names. `{in}` and `{out}` become `in.nec` and
  `out.txt`, e.g. `{in} {out}` or `-i{in} -o{out}`. For a program that asks for the
  names instead, leave this empty and tick **Type the file names when the program asks
  for them**.
- **Runs at once:** how many copies sweeps and the optimizer may start together
  (0 = one per processor thread); set 1 if the licence allows only one.

**Save and test** calculates the half-wave dipole with it. Then choose **External NEC**
under Calculate → Engine (or make it the start-up engine).

Each calculation writes the same NEC card deck as the NEC-2 engine (`GW`, `GN`, `LD`, `TL`,
`EX`, `FR`, `RP`, …) to `in.nec` in a temporary folder and runs the program there. The report
is read from `out.txt`, any other file the program writes, or what it prints, using the
NEC-2 report layout ("ANTENNA INPUT PARAMETERS", "RADIATION PATTERNS"). This is
**experimental**: it has been checked with stand-in programs, not with NEC-4 or NEC-5
themselves, whose input cards and report layout may differ. The last run's input,
output and messages are kept (Preferences shows where) so that a run that fails can be
looked at and the reader adjusted.

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
- **Wires lying on real ground** (height 0, such as radials laid on the soil) are
  modelled 1 cm under the surface, in the soil, with the ground's losses. A note
  in the results says so. Results differ a little from radials just above the
  ground, as they do in practice.
- **Wires that only stop on the surface of real ground**, with nothing lying on
  it or below it (no rod or radials), use a perfect-ground stand-in for the
  currents. A point contact into lossy earth can't be modelled. The warning
  suggests adding a rod or radials.
- **Speed:**
  - **Buried wires:** a vertical with 8 buried radials takes about 70 ms per
    frequency (sea water needs finer tables: about 0.3 s for 16 radials).
  - **Everything else:** for the 3-element Yagi (24-point sweep with gain and
    F/B, 12 threads), wiremom takes about 110 ms in free space, against 90 ms for
    MININEC and 170 ms for NEC-2. On perfect ground all three take about
    130 ms. Over real ground wiremom takes 350 ms, NEC-2 210 ms, and MININEC
    130 ms (but MININEC doesn't put real ground into the currents). A single
    calculation is faster with wiremom than with NEC-2 in every case.
- **Plates:** flat parallelograms only, above the ground (not in it). Large
  plates grow the problem quickly; a 1 m plate at 144 MHz is fine, but meshes of
  thousands of triangles take seconds.
- **NEC-2 engine:** plates are refused. Line loss is ignored, and a velocity
  factor becomes the equivalent electrical length. With automatic segmentation,
  NEC-2 gets uniform segments, because it is inaccurate next to the very short
  segments of the MMANA-style taper.
- **Second ground:** pattern only, as in NEC-2: currents, impedance and ground
  loss near the antenna come from the main ground, so put the station's own
  ground (e.g. the salt marsh it stands on) there. The cliff face doesn't reflect
  or diffract. Circular boundaries smaller than the Fresnel zone are
  approximate. In NEC-2, a straight boundary at a bearing other than 0° is
  modelled by turning the antenna, which NEC-2 files can't express: exported
  decks keep the full description in a `CM wireant ground2` line, and NEC-2
  itself reads the GD card as a boundary along +X.
- **Files:** transmission lines in EZNEC files are skipped on import. `.maa` files
  can't hold transmission lines or plates, and saved `.ez` files can't hold those or the
  second ground; saving says what was left out. Use `.nec` to keep everything.
- **Wire editor:** changes apply immediately. Press Calculate again for new
  results; the old results stay on screen until you do.

## Install on Windows or macOS

Download a package from the repository's Releases page (or the latest run on the Actions
tab: Build → Artifacts):

- **Windows:** `wireant-<version>-windows-x64.zip`. Unzip it anywhere and run
  `wireant.exe`. It isn't code-signed, so SmartScreen may warn the first time: choose
  **More info → Run anyway**.
- **macOS (Apple Silicon and Intel, macOS 11 or later):** `wireant-<version>-macos-universal.dmg`.
  Drag **wireant** to Applications. It isn't notarised, so open it the
  first time with right-click → **Open**, or allow it in System Settings → Privacy &
  Security. Open antenna files with File → Open (double-clicking them in Finder doesn't
  load them yet).

The NEC-2 engine is inside the program on all three systems. Settings are kept in
`%APPDATA%\wireant` on Windows and `~/Library/Application Support/wireant` on macOS.

To make the packages, push a version tag (`git tag v0.2.0 && git push --tags`): the Build
workflow then tests on all three systems and attaches the packages to a GitHub release.
Ordinary pushes test and package on Linux only, for quick results; to build the
Windows and macOS packages without a release, use Actions → Build → **Run workflow**.

## Install on Linux

A Linux package (`wireant-<version>-linux-x86_64.tar.gz`) comes with each release: unpack it
and run `./install.sh`. To build it yourself you need Rust (<https://rustup.rs>) and a C
compiler (`sudo apt install build-essential` on Debian/Ubuntu/Zorin). Cargo fetches the
wiremom engine from <https://github.com/tanganyika/wiremom> at the commit pinned in
`Cargo.lock`. Then:

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
| `src/mcp.rs` | MCP server (`--mcp`) |
| `src/settings.rs` | settings file (Preferences) |
| `docs/help/`, `src/help.rs` | help pages (Markdown, built into the program) and the Help window |
| `src/app.rs`, `src/plots.rs` | user interface and plots |
| `MECHANICS.md`, `mechanics.json` | where each formula and value came from |
| `tools/cc.sh` | optional C compiler for nec2c (`NEC2C_CC=tools/cc.sh`; falls back to `zig cc` when no `cc` exists) |
| `.github/workflows/build.yml`, `packaging/` | CI and the Linux, Windows and macOS packages |

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
