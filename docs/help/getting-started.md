# Getting started

wireant models antennas made of wires: dipoles, verticals, Yagis, quads, loops and
the like. You describe the wires, a feed point and the ground, press **Calculate**, and it
works out the feed impedance, SWR, gain, front-to-back ratio and radiation pattern.

## A first look

1. Choose **File → Examples → 3-element Yagi (14 MHz)**.
2. Open the **View** tab to see the antenna. The red dot on wire 2 is the feed point (the source).
3. Press **Calculate**. The status bar at the bottom shows the impedance, SWR, gain and F/B, and
   the wires are coloured by current: red where it is high, blue where it is low.
4. Open the **Calculate** tab for the full result and the horizontal and vertical patterns.
5. Open **Frequency sweep** and press **Run sweep** to see how SWR, impedance and gain change across the band.

## The tabs

- [Geometry](geometry.md): the antenna itself, with its wires, sources, loads, transmission lines,
  plates, ground and wire material.
- [View](view.md): a 3D picture of the antenna. You can also draw and edit wires with the mouse.
- [Calculate](calculate.md): runs the calculation at one frequency and lists the results.
- [Far field plots and 3D pattern](patterns.md): the radiation pattern in detail.
- [Frequency sweep](sweep.md): results across a range of frequencies.
- [Optimizer](optimizer.md): adjusts wires to reach a low SWR, high gain or high F/B.

The **Tools** menu holds the [resonance finder, match calculator and PDF report](tools.md).

## Building your own antenna

1. Choose **File → New**.
2. On the **Geometry** tab, set the **Frequency**.
3. Add wires, either by typing the ends of each wire or by drawing them on the View tab.
4. Add a source: choose the wire and where on it the feed point is.
5. Choose the ground and the wire material.
6. Press **Calculate**.

Coordinates are in metres. X and Y are horizontal and Z points up. Over ground, Z = 0 is the
ground's surface. The main direction for gain and F/B is +X (azimuth 0°).

Save your antenna with **File → Save**. See [Files and preferences](files.md) for the file types.

## Words used in the results

- **R + jX**: the feed impedance. R is the resistance in ohms, which includes the power radiated
  and lost. X is the reactance. When X is 0 the antenna is resonant.
- **SWR**: the standing wave ratio on a feed line whose impedance is the **SWR reference**
  (50 Ω unless you change it).
- **Ga dBi**: gain over an isotropic antenna in the best direction. **Gh dBd** is the same gain
  over a half-wave dipole, 2.15 dB less.
- **F/B**: front-to-back ratio, the gain forward over the strongest gain within the rear range.
- **Elev.**: the elevation angle of the strongest radiation above the horizon.
- **Segments**: each wire is split into short segments, and the current is found on each one.
  More segments give a more accurate result but take longer.
