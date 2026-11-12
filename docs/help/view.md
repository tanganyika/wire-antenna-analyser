# View

The View tab shows the antenna in 3D. Wire numbers, segment marks, sources (red dots) and loads
(green squares) are drawn on it. After a calculation, **Colour by current** colours each wire by its
current, from blue (low) to red (high).

## Moving around

- **Drag** to rotate, and **right-drag** or the scroll bars to move the view.
- The **wheel** zooms in or out at the mouse.
- **Click a wire** to zoom in there, and **right-click** an empty spot to zoom out.
- **Double-click**, or **Whole antenna**, shows all of it again.
- **Top**, **Front** and **Side** show the antenna from above, the front and the side.
  **Zoom out** steps back out.

The scale bar in the corner shows a length on the screen. Over ground, the grid shows the
ground's surface. With a second ground, its boundary is drawn in blue.

## Editing a wire

Right-click a wire to open the wire editor. Changes there apply immediately; press **Calculate** for
new results.

- **Length (m)**: type a new length, and choose which point to **keep**: the start, the centre or the end.
- The two ends, **Radius**, **Segments** (or **auto**), **Insulation** and **εr**.
- **Move the ends of wires joined to this one**: wires connected at an end that moves follow it,
  so junctions stay connected.
- **Delete wire**.

## Drawing wires

**Draw wires** turns on drawing mode. Choose between two modes:

- **New wire**: drag to draw a wire. If the drag starts on a wire end, the new wire joins it there.
  When you let go, the wire editor opens so you can type the exact length.
- **Adjust**:
  - Drag a wire end to change the wire's length along its own direction, or hold **Shift**
    to move the end anywhere. Wire ends joined there follow.
  - Drag the middle of a wire to move the whole wire.

Wires are drawn in a plane:

- **in plane**: choose **facing you**, the plane square to your line of sight, or a fixed
  **XY**, **XZ** or **YZ** plane. The plane goes through the point where the drag starts, or
  through the origin.
- **grid**: wire ends snap to a grid of the size you choose. They also snap to other wire ends.

While you drag, a readout shows the length and coordinates. **Ctrl+drag** still rotates the
view. **Undo** (Ctrl+Z) reverses drawing changes one at a time.
