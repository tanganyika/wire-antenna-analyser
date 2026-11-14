# Frequency sweep

The Frequency sweep tab calculates the antenna at many frequencies and plots the results.

- **From** and **to**: the frequency range in MHz. It starts at 3% below and 3% above the
  antenna's frequency.
- **points**: how many frequencies to calculate, spread evenly over the range.
- **Gain and F/B (slower)**: also work out the gain and front-to-back ratio at each frequency. Without
  it, only the impedance and SWR are calculated, which is quicker.
- **Engine** and **Threads**: as on the Calculate tab.

Press **Run sweep**. A bar counts the frequencies done, with the time so far and roughly how long
is left; **Stop** ends the sweep. Three plots appear:

- **SWR** against the SWR reference.
- **Impedance**: R and jX. The antenna is resonant where jX crosses zero.
- **Gain and front/back**: Ga in dBi on the left scale and F/B in dB on the right scale.

The vertical line marks the antenna's own frequency.

All engines run the frequencies in parallel. wiremom also shares its slow real-ground tables
between the frequencies of a sweep.

To find resonances and bandwidth exactly, use **Tools → Resonance finder**. See [Tools](tools.md).
