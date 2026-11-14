# Calculate

The Calculate tab works out the antenna at one frequency.

## Settings

- **Frequency (MHz)**: the frequency for this calculation.
- **Engine**: which calculation method to use. See [Engines](engines.md).
- **Ground**, **Wire material**, **Rear range for F/B** and **SWR reference**: the same settings as on
  the [Geometry](geometry.md) tab, repeated here for convenience.
- **Threads**: how many processor threads to use. All of them is fastest.

Press **Calculate**, or use the ▶ Calculate button on the View, Far field and 3D pattern tabs.
While it works, a bar counts the pattern directions (with NEC-2, its two runs); **Stop** ends it
after the step under way.

## Last result

The panel under the button shows the latest result in full:

- **Impedance** and **SWR** against the SWR reference.
- **Gain**, in dBi and in dBd.
- **F/B**.
- **Efficiency**: the share of the input power that is radiated into the sky. See Power budget below.
- **NEC efficiency**: the efficiency NEC-2 and 4NEC2 give, which does not count ground loss. See
  Power budget below.
- **Max at**: the direction of the strongest radiation, as azimuth and elevation.
- **Polarization**.
- **Segments**, and the number of unknowns solved.
- **Time**, split into matrix fill, solve and patterns.

Warnings appear here too. For example, the program warns about a wire that ends on real
ground, or a model that the chosen engine handles poorly.

## Results table

Every calculation adds a row to the **Results** table, so you can compare changes. **Clear** empties
it. The columns are:

| Column | Meaning |
|---|---|
| F (MHz) | frequency |
| R (Ohm), jX (Ohm) | feed impedance |
| SWR | against the SWR reference: green below 1.5, amber below 3, red from 3 |
| Gh dBd | gain over a half-wave dipole |
| Ga dBi | gain over an isotropic antenna |
| F/B dB | front-to-back ratio |
| Elev. | elevation of the strongest radiation |
| Eff. % | efficiency: the share of the input power that is radiated |
| Ground, Add H. | the ground used, and the height added to the antenna |
| Polar. | polarization: horizontal or vertical |
| ms, Threads | how long it took, and on how many threads |

The results history goes into the PDF report too.

## Power budget

The **Power** line under the table shows where the latest calculation's input power goes, for
example `Radiated 79.5 %   wires 0.6 %   ground 19.9 %   |   NEC efficiency 99.4 %`. Hold the mouse over it for how each part
is found.

- **Radiated** is the efficiency. It is the gain averaged over every direction, from the 3D pattern.
- **Wires**, **loads** and **lines** are the losses that the engine works out from the currents:
  - the wires' resistance, from the wire material;
  - the resistance in loads;
  - the loss in transmission lines.

  NEC-2 and NEC-5 give the wire and load losses as one figure, **wires and loads**.
- **Ground** appears over real ground. It is what is left once the radiated power and the other
  losses are taken away. Like NEC's figures, it counts the ground wave (the surface wave along the
  ground) as lost: that is what a low vertical sends along the ground rather than into the sky.

MININEC always finds the currents over perfect ground. So does wiremom when wires end on real
ground. For these, the ground figure is labelled **ground (reflection)**: it is only what the
ground's reflection takes. The loss in the ground close to the antenna, which can be large for a
vertical with few radials, is not included. To model it, use wiremom with buried radials or
ground rods (wires below z = 0).

### NEC efficiency

**NEC efficiency** is the share of the input power that is not lost in the antenna's own wires,
loads and lines. It is the efficiency in NEC-2's power budget, and the one 4NEC2 shows. NEC works
out its "radiated power" as the input power less those losses, so **power lost in the ground
counts as radiated**.

In free space and over perfect ground, NEC efficiency and Radiated are the same. Over real ground
they can be far apart. For example, an elevated quarter-wave vertical over average ground can show
an NEC efficiency of 99 % while radiating only about 28 % of its power into the sky. That is a
gain of about 0.9 dBi, rather than the 5.6 dBi it would have over perfect ground. Use **Radiated**
to judge how much power reaches the sky, and NEC efficiency to compare with figures from NEC
programs or to judge the loss in the antenna itself.

If the gain averaged over all directions comes to well over 100 % of the input power,
Radiated and Ground show **—** and a note explains. That happens when almost no power gets into
the antenna (an open feed, for example a feed line left out), and then the gain isn't reliable
either.

### Checking the model

In free space and over perfect ground, the parts should add up to 100 %. If they are a long way
off, the model needs more segments or has a problem, as with NEC's average gain test.

## Patterns

The horizontal pattern (seen from above) and the vertical pattern (seen from the side) are drawn
next to the results. They work as on the [Far field plots](patterns.md) tab.
