# Files and preferences

## Files

**File → Open** reads these file types:

- **MMANA** `.maa` files.
- **NEC** `.nec` decks, including 4nec2 symbols (`SY`), helices, arcs, `GM`/`GR`/`GX` moves and
  copies, `TL` lines and `SM`/`SC` plates.
- **EZNEC** `.ez` files.

**Save** writes the antenna back to its file in the same format. **Save as…** chooses the format
from the file name's ending (`.maa`, `.nec` or `.ez`); without one, it saves `.maa`. **Save as EZNEC** and **Save as NEC** export directly.

`.maa` files can't hold transmission lines or plates; use `.nec` to keep them. Transmission lines
in EZNEC files are skipped when they are opened.

**File → Examples** loads one of six ready-made antennas: a dipole, an inverted V, a Yagi, a
vertical, a ground plane and a quad.

## Preferences

**File → Preferences** sets:

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
such as Claude can build, calculate, sweep and optimize antennas with it. For Claude Code:

```
claude mcp add wireant -- /path/to/wireant --mcp
```

The [README on GitHub](https://github.com/tanganyika/wire-antenna-analyser#use-from-an-ai-assistant-mcp)
lists what the assistant can do.

## New versions

**Help → About** shows the version you have and where new versions can be downloaded.
