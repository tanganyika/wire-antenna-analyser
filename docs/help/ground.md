# Ground

Choose the ground on the Geometry or Calculate tab.

- **Free space**: no ground at all. This is good for comparing designs, and for VHF/UHF antennas
  high above the ground.
- **Perfect ground**: a perfectly conducting flat surface at Z = 0. A quarter-wave vertical on it
  behaves like an ideal ground-plane antenna.
- **Real ground**: soil with a relative permittivity (**Ground εr**) and conductivity
  (**Ground σ**, in mS/m). It absorbs some power and changes the pattern, most of all at low
  elevations.

**Presets** fills in typical values:

| Ground | εr | σ (mS/m) |
|---|---|---|
| Sea water | 81 | 5000 |
| Fresh water | 80 | 1 |
| Very good (pastoral, rich soil) | 20 | 30 |
| Average | 13 | 5 |
| Poor (rocky, sandy) | 5 | 1 |
| Very poor (city, industrial) | 3 | 1 |
| Wet ground (ITU) | 30 | 10 |

**Add height (m)** raises the whole antenna by that amount above the ground, without changing
the wire coordinates. This is a quick way to try different heights.

## Wires touching or in the ground

- A wire end at Z = 0 connects to the ground. Over perfect ground this is exact.
- Over real ground, a wire that just stops at the surface is a poor model: a single point contact
  into soil can't be calculated well. The program warns about it. Add a ground rod or radials.
- With the **wiremom** engine, wires can run below Z = 0 over real ground. Ground rods, buried
  radials and counterpoises are modelled in full, including the loss in the soil. Wires lying on
  the ground at Z = 0 are modelled 1 cm into the soil.
- NEC-2 and MININEC can't model wires in the ground.

## Second ground

Tick **Second ground** when part of the surroundings is different, such as the sea next to a
portable station, a salt marsh, or a cliff edge.

- **Boundary**: a **straight line** or a **circle round the origin**.
  - For a straight line, set the **Distance (m)** from the origin and the bearing (**toward bearing**)
    in which the second ground lies. The bearing is an azimuth: 0° = +X, 90° = +Y.
  - For a circle, set its **Radius (m)**. The second ground lies outside it.
- **εr** and **σ**: the second ground's own values, with presets.
- **Drop (m)**: how far its surface lies below the main ground, for a cliff or bank.
- **Transition**:
  - **Fresnel zone** (wiremom and MININEC): each ray's reflection is shared between the two
    grounds, so a nearby shore blends in smoothly.
  - **reflection point (NEC-2)**: each ray uses the ground where it reflects.

The second ground affects only the pattern, as in NEC-2. The impedance and ground loss come from
the main ground, so make that the ground the station stands on. The View tab draws the boundary
in blue.
