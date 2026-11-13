# Tools

The **Tools** menu holds the resonance finder, the match calculator and the PDF report.

## Resonance finder

The resonance finder looks for every frequency in a range where the reactance is zero, then
measures each resonance accurately.

1. Set **From**, **to** and the number of **points**. The range starts around the antenna's frequency.
2. Set the SWR for the bandwidth (**bandwidth at SWR**, usually 2).
3. Press **Find resonances**.

For each resonance it shows:

- The type: **series**, where R is low, or **parallel**, where R is high.
- R and the SWR against the SWR reference.
- The SWR bandwidth against the reference impedance, and the bandwidth once matched to the
  resonance's own resistance.
- Q, worked out as f·|dZ/df| / 2R.

Plots of SWR and of R and jX are shown too. **Use frequency** makes that resonance the antenna's
frequency. **Match…** opens the match calculator with its impedance.

The bandwidth edges are found by extra calculations, so they are accurate even when the band is
narrower than the point spacing. Large models take a while: about a second per calculation at
1,350 unknowns.

## Match calculator

The match calculator designs a match from an antenna impedance to a target, 50 Ω by default.
The impedance can be typed in, taken from the last calculation, or sent from the resonance finder.

It offers:

- **L networks**: every arrangement of one coil and one capacitor, series or shunt first, with
  values. Each is marked as low-pass or high-pass.
- **Gamma match**: the rod length and series capacitor, from the element and rod diameters and
  their spacing. This uses the ARRL Antenna Book's transmission-line model, which is approximate,
  so check the result by modelling the rod, or add it to the antenna (below), which does that
  for you.
- **Coax sections**:
  - A Regier series section of a chosen impedance (75, 93, 35 Ω and others).
  - A length of feeder to a point where the impedance is purely resistive, then a quarter-wave
    transformer.

  Lengths are given in electrical degrees, and in metres for the **velocity factor** you set.

### Adding a match to the antenna

Choose a solution with its round button, then press **Add to the antenna**. Use **From last
calculation** first, so the match is for this antenna's own impedance. The source moves to the
transmitter side of the match, and **↶ Undo** on the View tab (Ctrl+Z) takes it all out again.

- **L network**: the coil and capacitor become parts on a new line (see Transmission lines on the
  Geometry tab), from the feed point to a new short feed wire that carries the source.
- **Gamma match**: the driven element is split at the feed and at the strap, and the rod, strap and
  a short feed wire with the source and series capacitor are added as wires. The rod goes below a
  horizontal element. The formula is only a starting point, so the rod length and the capacitor
  are then found with the antenna's own calculation.
- **Coax sections**: the lines are added from the feed point, joined by short wires, with the
  source on the last one.

L networks and lines need the wiremom or NEC-2 engine. The parts are ideal (no loss).

## Antenna report (PDF)

**Tools → Antenna report (PDF)…** writes an A4 report containing:

- The antenna's settings.
- A picture of it as on the View tab, with wire numbers and current colours.
- Its loads, lines, plates and comment.
- The latest calculation and the results history.
- The horizontal and vertical patterns.
- The resonance finder's and match calculator's current results.

The save dialog suggests the antenna file's name ending in `.pdf`. Once saved, the report opens in
your PDF viewer.
