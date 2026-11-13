# Files and preferences

## Files

**File → Open** reads these file types:

- **MMANA** `.maa` files.
- **NEC** `.nec` decks, including 4nec2 symbols (`SY`), helices, arcs, `GM`/`GR`/`GX` moves and
  copies, `TL` lines and `SM`/`SC` plates.
- **EZNEC** `.ez` files.

**Save** writes the antenna back to its file in the same format. **Save as…** chooses the format
from the file name's ending (`.maa`, `.nec` or `.ez`); without one, it saves `.maa`. **Save as EZNEC** and **Save as NEC** export directly.

`.nec` files keep everything. Settings NEC has no card for, such as the SWR reference and
automatic segmentation, are kept in `CM` comment lines that other NEC programs skip. The
added height is folded into the wires' heights. `.maa` files can't hold transmission lines, plates or
current sources, and EZNEC files as saved here can't hold lines, plates or the second ground. If
you save an antenna that has any of these to a file that can't hold them, a note says what was
left out.

EZNEC files are read with their transmission lines (including stubs), L networks, transformers
and current sources. L networks and transformers become transmission lines with lumped parts. An
L network's parts keep the impedance they have at the file's frequency. A transformer becomes
two tightly coupled windings, very close to EZNEC's own model. Stubs end on a small wire of
their own, away from the antenna. Lines given in electrical degrees get the physical length for
the file's frequency. EZNEC puts sources, loads and line ends on segments, and this program puts
them where segments join. So each one goes to the centre of its EZNEC segment, and those wires
get twice as many segments, so there's a join there. Virtual segments aren't supported:
anything connected to one is skipped, with a note.

**File → Examples** loads one of six ready-made antennas: a dipole, an inverted V, a Yagi, a
vertical, a ground plane and a quad.

## Preferences

**File → Preferences** sets:

- What to **start with**: a blank antenna, or the antenna you last opened or saved.
  A file named on the command line is opened instead either way.
- The **engine** used when the program starts.
- The **External NEC program**. See [Engines](engines.md).

The folder you last opened or saved in is remembered as well. Settings are kept in:

- `~/.config/wireant` on Linux.
- `%APPDATA%\wireant` on Windows.
- `~/Library/Application Support/wireant` on macOS.

## Command line

```
wireant [antenna.maa|.nec|.ez]
```

opens an antenna. `--tab <name>` starts on a tab, for example `--tab sweep`.

## Using it from an AI assistant

`wireant --mcp` runs the program as an MCP server instead of opening a window, so an AI assistant
such as Claude can build, calculate, sweep and optimize antennas with it, and write the same PDF
report as **Tools → Antenna report**. For Claude Code:

```
claude mcp add wireant -- /path/to/wireant --mcp
```

The [README on GitHub](https://github.com/tanganyika/wireant#use-from-an-ai-assistant-mcp)
lists what the assistant can do.

## New versions

**Help → About** shows the version you have and where new versions can be downloaded.
