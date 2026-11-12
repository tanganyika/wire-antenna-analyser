NEC-5 test pack for Wire Antenna Analyser
=========================================

Thank you for trying this. It runs your NEC-5 on ten small test decks and collects what NEC-5
writes, so that Wire Antenna Analyser can learn to read NEC-5's output. Your NEC-5 program stays
where it is: nothing copies it or sends it anywhere. The only thing to send back is a zip of
NEC-5's output for these test decks.

What to do
----------

1. Unzip this pack anywhere, for example on the Desktop.
2. Double-click run-tests.bat. (Or drag your NEC-5 program onto run-tests.bat.)
3. When it asks, drag your NEC-5 program (the .exe you'd choose in EZNEC) onto the window, or
   type its full path, and press Enter.
4. It runs each deck in turn, one line per deck. It usually takes a few seconds in all; any deck
   that hasn't finished after 10 minutes is stopped and the next one started.
5. At the end it makes nec5-results.zip next to run-tests.bat. Please send that one file back.

If Windows says the file came from another computer, choose "More info" -> "Run anyway", or
right-click run-tests.bat -> Properties -> tick "Unblock" first.

What it does
------------

For each deck it does what EZNEC Pro does when it calls NEC-5: it puts the deck in your NEC-5
program's folder as WTP01.NEC (WTP02.NEC, ...) and runs

    "<your NEC-5 program>" "WTP01.NEC" "WTP01.OUT"

from that folder. It then moves WTP01.OUT into the "results" folder, deletes WTP01.NEC, and
records the program's exit code and anything it printed. If NEC-5 writes or changes any other
file in its folder (a log, say), a copy goes into the results too. Nothing is typed into NEC-5;
if it asks a question it gets end-of-input, so it can't sit waiting.

The results also note the NEC-5 program's file name, size, date and version information, so
that differences between NEC-5 builds can be told apart.

The decks
---------

  01  half-wave dipole, free space            the basic report layout
  02  the same dipole over real ground        GN 2 Sommerfeld ground
  03  quarter-wave vertical, perfect ground   GN 1
  04  3-element Yagi, real ground, aluminium  several wires, LD 5
  05  ground plane with 4 radials             wire junctions
  06  dipole: steel wire, loads, a line       LD 4 loads and a TL transmission line
  07  dipole with PVC-insulated wire          LD 2 insulation loading
  08  deck 01 with commas between numbers     does NEC-5 accept both layouts?
  09  deck 01 written as EZNEC writes it      EX 4 source at a wire end, LD 5 with 6 fields
  10  a deck EZNEC Pro wrote for NEC-5        a known-good control

Decks 01-08 are exactly what Wire Antenna Analyser sends to an external NEC program today. Some
of them may well fail on NEC-5: that is useful to know too, and nothing is harmed.

Wire Antenna Analyser: https://github.com/tanganyika/wire-antenna-analyser
