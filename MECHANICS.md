# Mechanics taken from the original

Source: the installer `mmbasic.exe` (MMANA-GAL_Basic 3.5.3.82, built with Actual Installer)
contains three programs and 722 example antennas:

| File | What it is | Analysed? |
|---|---|---|
| `MMANAGALBasic35.exe` | main program (Delphi), sha256 `ab0b6122…70459d7` | **Code: no.** Opened with core_target_open: `native_strings_find` finds **0 strings** (all code and text are compressed), and 47 of its 48 Delphi forms are compressed or encrypted (the readable `TACAL*`/`TFARSETDLG` blocks don't start with `TPF0`). Behind ASPack is a copy-protection layer (registration, hardware ID, sockets). Getting past it means defeating copy protection, so I stopped there. |
| `Language/english.mmn` etc. | the main program's screen texts (plain text) | Yes: labels, materials, limits, messages (§6) |
| `FFSolver.exe` | "FF Solver GAL v. 2.6 B20 06.2015" (MSVC 2008, not packed), sha256 `5f432fd8…5d71111b` | Yes: everything below with an address comes from it |
| `MMFarField.exe` | far-field viewer | not needed |
| `ANT/**/*.maa` | 722 example antennas | used to check the file format: every one loads and 720 solve |

All addresses are virtual addresses in `FFSolver.exe` (image base 0x400000).
Pseudo-code came from the Deep engine (Ghidra); "derived" means I read the behaviour from
x87 pseudo-code that was partly garbled, so the exact branch conditions are my reading of it.

## 1. Method and constants

FFSolver is a **MININEC-style** pulse solver, the same family as the rebuild's engine
(triangle current with pulse charge, delta-gap source, image method for perfect ground).

| Value | Where | Used in the rebuild |
|---|---|---|
| wavelength λ = 299.792458 / f(MHz) m | 0x408590 (`DAT_00425a70`) | `Antenna::lambda`, `mom::quick` |
| k = 2π/λ (6.283185307179586) | 0x408590 (`DAT_00425b10`) | `mom::solve` |
| MININEC G0 = 29.979221 (η0/4π) | 0x408590 (`DAT_00425d60`) | not needed (SI units used) |
| scalar-potential factor 4.77783352·λ | 0x408590 (`DAT_00425d50`) | equivalent to 1/(jωε0) in `mom::solve` |
| small-distance thresholds 0.001·λ and 0.0001·λ | 0x408590 (`DAT_00425d58`, `DAT_00425ac8`) | reduced-kernel radius handles this |
| default frequency 14.15 MHz (λ = 21.1867 m) | `DAT_00425af8/af0` | default examples |
| most pulses: 40 000 (4 000 in its small mode) | 0x408a40 | `mom::build_mesh` refuses more than 40 000 |
| ends at z ≤ 0 (after "add height") connect to ground | 0x407ab0 | `mom::build_mesh` |
| wires with zero radius or zero length are skipped | 0x406cc0, 0x407ab0 | `mom::build_mesh` (warning instead of an error) |

## 2. Segmentation (per wire, the 8th wire column)

Dispatch at **0x4064b0** on the wire's segment value:

| Segment value | Function | Rule |
|---|---|---|
| > 0 | **0x405390** | n = value; if L/n < 2r then n = ⌊L/(2r)⌋ (no segment shorter than the diameter); if λ/L > **1500** (`DAT_00425b38`) then n = 2; odd n is rounded **up to even**; n ≥ 2 |
| 0 | **0x405460** | n = ⌊L·DM2/λ⌋, reduced by 2 while L/n < 2r; then even and ≥ 2 |
| −1 (auto) | **0x405540** | see below |
| −2, −3, < −10000 | 0x405c20, 0x406070, 0x405540 with DM2 = (value >> 16) & 0x7fff | not rebuilt; treated as −1 (only 2 of 4 000 example wires use −2) |

Auto segmentation (0x405540), using the file's `*** Segmentation ***` line `DM1, DM2, SC, EC`:
1. If λ/DM1 < 2r, DM1 drops to ⌊λ/(2r)⌋ and DM2 to at most DM1.
2. A wire shorter than λ/DM1 gets 2 segments.
3. Each end gets **EC segments of λ/DM1**, then single segments of λ·SC^k/DM1 (k = 1, 2, …)
   while they are still shorter than λ/DM2.
4. The middle gets n = ⌊middle/(λ/DM2)⌋ rounded **down to even** segments; if n < 5 and
   λ/DM2 < 0.85 × (middle/n) (`DAT_00425b20` = 0.85), n += 2.
5. If the middle doesn't fit (n < 2), DM2 is doubled and the taper is redone; if DM2 reaches
   DM1, the wire is cut uniformly (rule for 0).

So **EC is a count** (shortest segments at each end), not an on/off switch.
Rebuilt in `mom::wire_segment_lengths`. Confidence: derived.

Defaults: the example files use `400, 40, 2.0, 1` (189 of 330 English files) far more than
anything else (`800, 80, 2.0, 1` is next), so that is the rebuild's default.

## 3. Loads (`*** Load ***` lines: `position, type, a, b, c`)

From **0x408200** and **0x407460** (f in MHz, L in µH, C in pF):

| Type | Values | Impedance |
|---|---|---|
| 1 | R, X | R + jX |
| 0, only L | L, 0, Q | X = 2π·f·L, R = X/Q (R = 0 when Q = 0) |
| 0, only C | 0, C, Q | X = −1/(2π·f·C·10⁻⁶), R = \|X\|/Q |
| 0, L and C | L, C, Q | **parallel trap**: Y = 1/(R_L + jX_L) + j·2π·f·C·10⁻⁶ with R_L = X_L/Q, Z = 1/Y |
| 2 | frequency table | not rebuilt (none of the 722 examples use it) |

Constants: 2π (`DAT_00425b10`) and 2π·10⁻⁶ (`DAT_00425b08`).
Rebuilt in `Load::impedance`; the Geometry tab now asks for L, C and Q (the old "parallel"
box and series R were my guess and are gone).

## 4. Far field

**0x4093e0** (grid) and **0x408d20** (field in one direction):
- Grid: elevation steps × azimuth steps; azimuth 0° is +X, counter-clockwise; zenith angle starts at 90° (horizon).
- Gain per polarization and total: G = 10·log10(|E|²·0.033356/P) (`DAT_00425d40` = 0.033356 ≈ 1/29.98),
  with **−999 dB** (`DAT_00425d20`) when |E|²·… ≤ 10⁻³⁰ (`DAT_00425d28`). Rebuilt in `mom::db`
  (same normalisation via 4πU/P_in).
- Perfect ground: image currents (horizontal parts reversed). Same in `Radiator::gain`.
- Real ground: reflection coefficients from ε_c = εr − jσ/(2π·f·10⁶·ε0) (`DAT_00425d48` =
  2π·ε0·10⁶ = 5.5606·10⁻⁵), the classic Fresnel form. Same in `Radiator::gain`.
  The original also supports several ground zones by distance and a radial wire screen
  (`DAT_0043e87c`, `DAT_0043e884`); the rebuild has one zone and no screen.

## 5. File format (`.maa`) checked against the 722 examples

- Line 1 name, then `*`, then frequency (MHz).
- Section headings are **English or Russian** (Windows-1251), e.g. `***Источ.***`, `***Нагрузка***`,
  `***Автосегм***`, `* Провода *`; some use a single `*`. The reader goes by name and falls back
  to the fixed order wires, source, load, segmentation, G/H/M/R/AzEl/X. Comments `###…###`.
- Wire: `X1, Y1, Z1, X2, Y2, Z2, R (m), Seg` (Seg −1 in 3 998 of 4 012 example wires).
- Source: header `count, 1`; lines `wNc|wNb|wNe[k], phase, volts`. A trailing number (`w1e1`, `w2b2`)
  appears in 4 files; its meaning isn't known, so the rebuild places it at that end/centre.
- Load: header `count, 1`; type 0 (245 lines) or type 1 (89 lines), see §3.
- G/H/M/R/AzEl/X: ground (0 free, 1 perfect, 2 real), add height, material code (0–4 seen),
  SWR reference Ω, view azimuth, view elevation, X.
- Ground εr and σ are **not** in the file. The real defaults live in the protected main program,
  so 13 and 5 mS/m are still estimates.
- Material codes: 0 = lossless and 1 = copper are certain from the examples; what 2, 3 and 4 mean
  can't be read without the main program, so they stay estimates (aluminium, brass, steel).

## 6. From the main program's language file (`Language/english.mmn`) and `whatsnew.txt`

These are the original's own words for its controls, so they settle names and lists
(not formulas):

| What | Original text | Used in the rebuild |
|---|---|---|
| Tabs | Geometry, View, Calculate, Far field plots (+ "3D FF" button, "Plots" window with Z / SWR / Gain/FB) | same tab names; 3D pattern and frequency sweep as tabs |
| Results grid | `No., F (MHz), R (Ohm), jX (Ohm), SWR, Gh dBd, Ga dBi, F/B dB, Elev., Ground, Add H., Polar.` | same columns and order (`results_table`) |
| Polarization | `hori.` / `vert.` | same |
| Ground | Free space / Perfect / Real; real ground has a media table (Dielec., Conduct (mS/m), R or X, Height) and extra radials (number, radius) | Free / Perfect / Real with one medium; zones and radials not rebuilt |
| Wire material (code = index) | 0 No loss, 1 Cu wire, 2 Cu pipe, 3 Al wire, 4 Al pipe, 5 Fe wire, 6 Fe pipe, 7 User wire, 8 User pipe (user: Resistivity, Permeability) | `Material` enum in this order; user resistivity/permeability fields. Conductivities (Cu 5.8e7, Al 3.5e7, Fe 1.0e7 S/m with μr 100) are **estimates**; pipe = wire |
| Auto segmentation | "DM1: the interval of tapering start (= lambda/DM1)", "DM2: … end (= lambda/DM2)", "SC: the easing parameter (multiplier), **1 < SC <= 3**", "EC: the number of segments at tapering-end point" | confirms §2; SC limited to 1.01–3 |
| F/B | "Rear range of F/B: Azimuth … deg, Vertical … deg" = the `AzEl` fields of the G/H/M/R/AzEl/X line (120, 60 in 307 of 330 examples; also 60/20, 20/40, 0/0) | `Antenna::fb_az/fb_el`, read and written in .maa; F/B = front gain − largest gain in that rear range (`mom::rear_max`) |
| Under-ground error | "Antenna or part of antenna is under ground. Increase height!" | same message |
| Limits (3.5.3) | "segments (max.) increased up to 10000", "wires … 600", "sources … 100" | not enforced: the rebuild's multi-threaded solver goes up to the solver's 40 000 pulses |
| Far-field CSV header | `ZENITH(DEG),AZIMUTH(DEG),VERT(dBi),HORI(dBi),TOTAL(dBi)` | not yet exported |

## Not taken from the original

- The main program's own impedance solver, optimizer, default ground values, colours and screens
  (protected, see above).
- Multi-threading is the rebuild's own: matrix fill, LU elimination, pattern scans, sweeps and
  optimizer trials run on all cores (rayon).

## 7. Second engine: NEC-2 (added at the person's request)

NEC-5 (and NEC-4) are licensed by Lawrence Livermore National Laboratory and can't be downloaded
freely, so the rebuild bundles the **public-domain NEC-2** engine instead: `nec2c` 1.3.1
(Debian source `nec2c_1.3.1.orig.tar.bz2`; its README states "nec2c is Public Domain, same as the
original FORTRAN source"). Source is in `nec2c/`; `build.rs` compiles it with `tools/cc.sh`
and the program embeds the executable.

- Pick it under **Engine** on the Calculate or Frequency sweep tab; the optimizer uses the same choice.
- Each calculation writes a NEC deck and runs nec2c as its own process; sweeps and optimizer trials
  run many processes at once (24-point Yagi sweep: 3.0 s on 1 thread, 0.5 s on 12).
- Deck: the same segmentation as §2, one `GW` per run of equal segments (a centre feed that would
  fall on a segment boundary gets an odd middle run, because NEC feeds at segment centres);
  `GE 0/1`; `GN 1` perfect or `GN 2` (Sommerfeld-Norton) real ground with εr and σ;
  `LD 5` wire conductivity (σ/μr for iron and user materials); loads as `LD 4` R+jX at the
  frequency; sources `EX 0`; patterns from a 3° `RP` grid, then 1° cuts through the maximum.
- Agreement with the built-in solver (`nec::tests`): dipole 72.5−j0.2 vs 73.0+j1.3 Ω, quad
  143.8+j4.3 vs 142.8+j4.5 Ω, gains within 0.3 dB; over real ground NEC-2 differs more because it
  models the ground near the antenna too.

## 8. 3D far-field viewer (`MMFarField.exe`, not packed, sha256 `de359d7b…a40ca34`)

Opened with core_target_open; found through its strings ("Cursor Az angle = %s,  Ev angle = %s,
G = %.1f dBi", "Min of dB scale") and its published handler table (`Sc1Click` 0x407708,
`Sc4Click` 0x407c2c, `MinDBUDChangingEx` 0x407a5c, settings writer 0x403a34).

| Rule | Where | Used in the rebuild |
|---|---|---|
| Scale menu: ARRL / dB / Linear; only the dB scale uses "Min of dB scale" | 0x407708, 0x407c2c | 3D pattern tab: `Scale3D`, min-dB control shown for dB only |
| Default min of dB scale **−40 dB** | `DAT_004b7238`, saved by 0x403a34 | `plots::MIN_DB_DEFAULT` |
| Min-dB spinner: ±10 dB below −20 dB (`DAT_00407c24`), ±5 dB from −20 to −10 dB (`DAT_00407c28`), ±1 dB above; kept within −100…−1 dB | 0x407a5c | `plots::step_min_db` (tested) |
| Cursor readout: "Az angle, Ev angle, G = %.1f dBi", one decimal, never below **−100 dBi** | 0x406480 | polar markers and 3D hover now show one decimal and the −100 dBi floor (`plots::CURSOR_FLOOR_DB`) |
| Gradient correction (gamma) default 3.0 | `DAT_004b7224` | not used (the rebuild's colours are its own) |

The exact radius formulas of the ARRL and Linear scales weren't found as constants (no 0.89 in the
file); the rebuild keeps the ARRL-style 0.89^(−dB/2) radius and uses field strength (10^(dB/20))
for Linear. Those two are estimates.
| Right-drag pans by Δx·**2.8**/window width and Δy·**2.5**/window height scene units (left-drag rotates) | mouse handlers 0x404eac (down), 0x404f14 (move), 0x404ef0 (up); constants 0x4050c0, 0x4050c8 | 3D pattern tab right-drag (`plots::PAN_X_UNITS`, `PAN_Y_UNITS`) |

The antenna View tab's click-to-zoom, zoom-at-mouse and scale bar are the rebuild's own: the
original's antenna view is inside the protected main program, and the 3D viewer has no
mouse-wheel or click-zoom handler (only `FormMouseDown/Move/Up`).

## 9. NEC files and wire insulation (the rebuild's own additions)

**.nec files** (`nec_file.rs`): File ▸ Open reads `.maa` and `.nec`; File ▸ Save as NEC writes a deck.
Read: CM/CE (first comment = name), 4nec2 `SY` symbols with formulas (`+ − * / ^`, sqrt/sin/cos/tan/
atn/exp/log/abs/int, angles in degrees, `pi`), GW, GA (arc → straight wires), GH (helix → straight
wires, e.g. coils), GM (move/rotate/copy), GR (copies around Z), GX (reflection), GS (scale), GE/GN
(free, perfect, real ground with εr and σ), EX 0 voltage sources, LD 0/1 series/parallel RLC (new
`LoadKind::Rlc`), LD 4 R+jX, LD 5 conductivity (→ Cu/Al/user material), FR (first frequency).
Anything else is listed in a note when the file opens. Write: one GW per wire (centre feeds get an
odd segment count), GE/GN, LD 5, LD 2 for insulation, loads (traps as LD 1 with R = Q·X_L, series
L or C as LD 0 with R = |X|/Q at the design frequency), EX 0, FR, RP, EN. In `.maa` files, NEC-style
R-L-C loads are stored as R+jX at the design frequency, and insulation in an extra
`*** Insulation ***` section (wire, thickness m, εr).

**Wire insulation** (per wire: thickness and εr; presets PVC 3.5, polyethylene 2.3, PTFE 2.1,
rubber 3.0, enamel 3.6 — typical values): modelled as a distributed series inductance
L′ = μ0/(2π)·(1 − 1/εr)·ln((a+t)/a) per metre (`Wire::insulation_l_per_m`), the standard
thin-insulation model. The built-in solver adds jωL′ along each insulated segment; NEC-2 gets
`LD 2` cards. 0.5 mm PVC on a 1 mm-radius 14 MHz dipole raises X by about 27 Ω in both engines
(resonance about 2.5 % lower). The VF column is a display estimate, 1/√(1 + L′/L0) with
L0 = μ0/(2π)·ln(L/a).

## 10. EZNEC .ez files (the rebuild's own addition)

`ez.rs` reads and writes EZNEC's binary .ez format. The layout comes from the published notes
collected by the open-source ezparse project (GPL; read for the format only, no code copied) and
was checked against 52 real files (all read, all calculate): a 170-byte global record (check
value 1304 at byte 63, frequency, title, counts, ground type F/P/R, wire ρ and μ, SWR Z0, load type
Z/R/L), one 170-byte record per index holding wire i, source i and load i, two more 170-byte
records, then tagged blocks (12 = insulation εr/thickness/loss tangent per wire, 13 = ρ/μ per wire).
Coordinates and diameters are stored in metres; AWG gauge is used when no diameter is stored
(d = 0.005·92^((36−ga)/39) inch). Sources: percent along the wire; EZNEC current sources are read
as voltage sources. Loads: R+jX, series/parallel RLC, and traps (R in series with L, parallel C →
the MMANA-style L/C/Q load). Not supported yet: transmission lines, transformers, L-networks,
Laplace loads, radial ground screens and a second ground medium. A note lists them when a file
uses them, and those antennas won't calculate correctly. Writing uses metres, mm diameters,
EZNEC's "MININEC" ground analysis when the built-in engine is selected and "high accuracy" for
NEC-2, block 12 when wires are insulated, and block 13 for wire loss.

## 11. Installing (from the original installer's settings)

The installer `mmbasic.exe` (Actual Installer 9.2) carries its settings as plain text in
`aisetup.ini`: it installs into its own folder (`InstallDir=c:\MMANA-GALBasic3`), creates a
program-group shortcut to the program and an "Uninstall <AppName>" shortcut (`[Shortcuts]` 0 and 1),
a desktop shortcut (`[Shortcuts]` 2), and its finish page offers to start the program, ticked by
default (`LaunchFile=1`, `LaunchFileChecked=1`). It registers no file types. It also installs the
722 example antennas (`ANT\…`), which belong to the MMANA-GAL authors and aren't redistributed here.

`install.sh` does the Linux equivalent per user (no root): program in `~/.local/bin`, a menu entry,
an "Uninstall Wire Antenna Analyser" menu entry, a desktop shortcut (`NO_DESKTOP=1` skips it), and
a "Start it now? [Y/n]" question at the end (`NO_LAUNCH=1` skips it). Extras the original doesn't
have: an icon, and .maa/.nec/.ez files opening with the program. `uninstall.sh` removes all of it.
