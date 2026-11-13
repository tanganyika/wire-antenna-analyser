# nec-stub

A stand-in for a NEC program, used to find out how another program, such as EZNEC Pro's NEC-5
option, runs NEC: the command line, the folder it runs in, any file names it types in, and the
input deck it writes. It calculates nothing.

## Using it with EZNEC Pro

1. Copy `nec-stub.exe` to the Windows computer, e.g. to `C:\nec-stub\nec-stub.exe`.
2. In EZNEC Pro, choose the NEC-5 (or NEC-4) option and pick `nec-stub.exe` as its executable.
   If EZNEC insists on a particular file name, rename the copy to match.
3. Open an antenna and run a calculation, e.g. **FF Plot** or **SWR**. Try a few kinds of
   antenna: over real ground, with loads, with insulated wire. Each run is recorded separately.
4. EZNEC will then report an error or missing results. That is expected, because the stub
   does no calculation.
5. Look in the **nec-stub-log** folder on the Desktop. Each run has a folder holding:
   - `report.txt`: the command line exactly as given, the working folder, anything typed in
     reply to its prompts, and a list of the folder's files;
   - `files\`: copies of the input deck and of anything else written in the last 30 minutes.

Send back the whole `nec-stub-log` folder.

Switch EZNEC back to the real NEC afterwards.

## What it does not show

It shows only what the calling program sends. The NEC-5 report that comes back, which is the
part that wireant has to read, can only come from a real NEC-5 run.

## Building

```sh
cargo test --release
# Windows 64-bit, from Linux (needs the target and zig: rustup target add x86_64-pc-windows-gnu,
# cargo install cargo-zigbuild)
cargo zigbuild --release --target x86_64-pc-windows-gnu
```

It uses only Rust's standard library and isn't part of the main program's build.
