# NEC-5 test pack

A zip for someone with a licensed NEC-5 to run: `run-tests.bat` runs their NEC-5 on each deck in
`decks/` the way EZNEC Pro calls it (`"nec5.exe" "WTP01.NEC" "WTP01.OUT"` from the program's
folder, as found with [nec-stub](../nec-stub)) and zips the output and messages. NEC-5 itself
never leaves their computer. `README.txt` is the recipient's guide; it lists the decks.

- Decks 01-07 were captured from the External NEC engine (a script set as the NEC program copied
  `in.nec`), so they are exactly what Wire Antenna Analyser sends. Recapture them if the deck
  writer in `src/nec.rs` changes.
- 08 is 01 with commas, 09 is 01 written in EZNEC's NEC-5 style, and 10 is a deck EZNEC Pro 7.0.4
  wrote for NEC-5.

`run-tests.ps1` works with Windows PowerShell 5.1 and PowerShell 7 (`-TimeoutMinutes` shortens
the per-deck limit for testing). To make the zip to send:

```sh
./make-zip.sh   # writes dist/wireant-nec5-testpack.zip
```
