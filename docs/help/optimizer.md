# Optimizer

The optimizer changes chosen wires, step by step, to improve the antenna at its frequency.

## What to change

Each line is one thing the optimizer may change. Choose a wire and what to change, then click **Add**:

- **Length (joined ends stay)**: makes the wire longer or shorter. Ends joined to other wires or to
  the ground stay where they are. When neither end is held, the wire grows from its centre.
- **Move along X**, **Move along Y**, **Move along Z**: moves the whole wire. Wires joined to it
  follow, so junctions stay connected.
- **X1**, **Y1**, **Z1**, **X2**, **Y2**, **Z2**: moves one end. Wire ends joined there follow.

By default, the first three wires' lengths are listed. 🗑 removes a line.

## Goals

Each goal has a weight. 0 ignores it, and a higher weight makes it count more:

- **Low SWR**: against the SWR reference.
- **jX close to 0**: towards resonance.
- **High gain**.
- **High F/B**.

**Most steps** limits how long it runs. The line underneath shows the frequency and the SWR
reference it works to.

## Running it

Press **Start**. The **Progress** plot shows the score at each step (lower is better), and the table
lists R, jX, SWR, gain and F/B as it goes. A bar counts the steps against **Most steps**; it
usually finishes sooner. **Stop** ends it early.

When it finishes, the result is described underneath. If it found a better antenna, click
**Use optimized antenna** to replace yours with it, then press **Calculate**. **Go back to the
antenna before optimizing** undoes that.

If nothing better was found, it says so. This can happen when the antenna is already as good as
those changes can make it, or when the goals pull in different directions. Try other wires or
other goals.

## Tips

- To tune a dipole or vertical to resonance, change its length with **Low SWR** and
  **jX close to 0**.
- For a Yagi, change the element lengths and the spacing (**Move along X**), and give
  **High gain** or **High F/B** some weight.
- The optimizer searches near the starting point. Start from a sensible antenna.
