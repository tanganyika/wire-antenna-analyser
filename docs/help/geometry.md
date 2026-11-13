# Geometry

The Geometry tab holds the whole antenna. Changes take effect straight away; press **Calculate**
again to see their results.

At the top you give the antenna a **Name** and set its **Frequency (MHz)**. The wavelength (λ) is
shown next to it.

## Wires

Each row is one straight wire:

- **X1, Y1, Z1** and **X2, Y2, Z2**: the two ends, in metres.
- **R (mm)**: the wire's radius. This is half its diameter.
- **Seg**: how many segments the wire is split into. With **auto** ticked, the number comes from
  the automatic segmentation settings (see below).
- **Length (m)**: worked out from the ends.
- **Insulation (mm)**, **εr**: the thickness of the insulation and its relative permittivity. Insulation
  slows the wave on the wire, so an insulated wire resonates lower than a bare one. **VF** shows the
  velocity factor this gives in air. With the wiremom engine, insulation on wires in the ground or
  lying on it (buried or surface radials) is modelled for the soil around it. There the sleeve
  keeps the wire out of contact with the soil and slows the wave much more, so an insulated buried
  wire resonates far shorter than in air. The other engines treat insulation as it acts in air.
- 🗑 deletes the wire. Any sources and loads on it go with it.

Wires connect where their ends meet exactly, within a micrometre. A wire end at Z = 0 over
perfect or real ground connects to the ground.

**Insulation for all wires** sets the same insulation on every wire in one go, with a choice of
PVC, polyethylene or PTFE. **Remove all** takes it off again.

## Sources

A source is the feed point. Choose the **Wire** and the **Position** on it: begin, centre, end, or
a custom fraction of the way along. **Phase**, **Value** and **Type** matter only when there are
several sources, as in a phased array. The impedance and SWR shown are for the first source.

**Type** is **voltage** or **current**. A voltage source keeps its voltage (the value in volts),
and a current source its current (the value in amps), whatever impedance it meets. Phased arrays
are usually designed for given currents in their elements, so feed them with current sources.
With current sources, each calculation solves the antenna once per source first, to find the
voltages that give those currents, so it takes a little longer.

## Loads

Loads are components placed on a wire:

- **R+jX**: a fixed resistance and reactance.
- **L/C**: an inductor, a capacitor or both. With both, they are in series, or in parallel (a trap)
  when **parallel** is ticked. **Q** adds the coil's loss; 0 means lossless.

## Transmission lines

These are feed lines, phasing lines and stubs between two places on wires. They work with the
wiremom and NEC-2 engines only. For each line you set:

- **Z0**: the characteristic impedance.
- **Length**: 0 means the straight distance between the two ends.
- **Vel. factor**: about 0.66 for solid-PE coax and 0.8 for foam.
- **Loss**: in dB per 100 m at the calculation frequency. NEC-2 ignores loss.
- **Reversed**: the conductors are swapped at the far end, a half twist.
- **Parts**: coils and capacitors that the match calculator added. A part can sit across either
  end, or join the two ends in place of the line, and then the line's own values are unused.
  Delete the line to remove them. In `.nec` files they are TL shunt admittances and NT cards.

## Plates

Plates are flat conducting sheets, such as a reflector, a ground screen or a car-body panel. They
work with the wiremom engine only. Each is a parallelogram, given by one corner and the two edges
that meet there. **Divisions** sets how finely it is meshed; 0 means automatic, about λ/10. A wire
that ends on a plate connects to it, so a monopole can be fed against a plate.

## Ground and wire

- **Ground**: **Free space**, **Perfect ground** or **Real ground**. See [Ground](ground.md).
- **Wire material**: copper, aluminium or iron, as wire or pipe, or **No loss**. **User wire** and
  **User pipe** take your own resistivity and permeability.
- **Rear range for F/B**: how wide the rear region is, in azimuth and in elevation, when looking
  for the strongest rear lobe.
- **SWR reference (Ω)**: the feed line impedance used for SWR.

## Automatic segmentation

Automatic segmentation puts short segments near wire ends and junctions, where the current changes
fastest, and longer ones in the middle of a wire:

- **Shortest segment λ /** and **Longest segment λ /**: the segment length limits, as fractions of a wavelength.
- **Growth factor**: how quickly the segments lengthen away from an end.
- **Shortest segments at each end**: how many segments of the shortest length come first.

The NEC-2 engine always uses evenly spaced segments, because NEC-2 is inaccurate next to very
short segments.

## Comment

Free text saved with the antenna. It also appears in the PDF report.
