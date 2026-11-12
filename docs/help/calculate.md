# Calculate

The Calculate tab works out the antenna at one frequency.

## Settings

- **Frequency (MHz)**: the frequency for this calculation.
- **Engine**: which calculation method to use. See [Engines](engines.md).
- **Ground**, **Wire material**, **Rear range for F/B** and **SWR reference**: the same settings as on
  the [Geometry](geometry.md) tab, repeated here for convenience.
- **Threads**: how many processor threads to use. All of them is fastest.

Press **Calculate**, or use the ▶ Calculate button on the View, Far field and 3D pattern tabs.

## Last result

The panel under the button shows the latest result in full:

- **Impedance** and **SWR** against the SWR reference.
- **Gain**, in dBi and in dBd.
- **F/B**.
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
| Ground, Add H. | the ground used, and the height added to the antenna |
| Polar. | polarization: horizontal or vertical |
| ms, Threads | how long it took, and on how many threads |

The results history goes into the PDF report too.

## Patterns

The horizontal pattern (seen from above) and the vertical pattern (seen from the side) are drawn
next to the results. They work as on the [Far field plots](patterns.md) tab.
