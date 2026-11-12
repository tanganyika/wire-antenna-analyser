# Engines

The engine is the method used to calculate the antenna. Choose it under **Calculate → Engine**,
or set the start-up engine in **File → Preferences**. All engines give the same results for a
simple antenna in free space. They differ over real ground and in what they can model.

- **wiremom** (the default): the built-in solver. It handles real ground fully, wires in the ground,
  transmission lines with loss, and plates. It runs on all processor cores.
- **MININEC**: the method MMANA uses. Real ground only affects the pattern. The currents and
  impedance are worked out over perfect ground. Use it to compare with MMANA.
- **NEC-2**: the standard NEC-2 program, built in. It is the reference to compare with EZNEC,
  4nec2 and other NEC tools. It can't model plates or wires in the ground, and it treats
  transmission lines as lossless.
- **External NEC** (experimental): your own copy of NEC-4, NEC-5 or another NEC program.

## Which to use

- **Free space or perfect ground**: any of them. They agree closely.
- **Low antennas over real ground**: wiremom or NEC-2. MININEC's impedance can be noticeably off.
- **Ground-mounted verticals, buried radials, ground rods**: wiremom only.
- **Feed lines and stubs with loss**: wiremom.
- **Plates**: wiremom.

## External NEC

NEC-4 and NEC-5 are licensed and can't be included, but if you have a copy the program can run
it, as EZNEC Pro does. Under **File → Preferences → External NEC program**:

- **Program**: the NEC executable.
- **Arguments**: how it takes its file names. `{in}` and `{out}` become the input and output files,
  for example `{in} {out}` or `-i{in} -o{out}`.
- If the program asks for the file names instead, leave **Arguments** empty and tick **Type the
  file names when the program asks for them**.
- **Runs at once**: how many copies sweeps and the optimizer may run together. 0 means one per
  processor thread. Set 1 if the licence allows only one.

**Save and test** calculates a half-wave dipole with it. Then choose **External NEC** as the engine.

Each calculation writes a NEC-2 card deck and runs the program on it, then reads its report. This
has not yet been tried with real NEC-4 or NEC-5, whose cards and report layout may differ. The
last run's files are kept so that a failed run can be looked into; Preferences shows where.
