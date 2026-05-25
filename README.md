# DEET-Debugger

`deet` is a small command-line debugger implemented in Rust for Linux `x86_64` programs with DWARF debug information.

This repository contains the debugger source code in [`deet/`](./deet/).

## What This Project Does

`deet` supports a subset of classic debugger functionality:

- Launching and controlling a target process with `ptrace`
- Setting and removing software breakpoints
- Continuing and single-stepping execution
- Printing register values
- Reading memory at a target address
- Printing stack backtraces
- Resolving functions, source lines, and variables from DWARF data
- Inspecting local variables and parameters at the current stop point

The current implementation is intended for educational use and debugger internals practice, not as a production debugger.

## Repository Layout

```text
proj-1/
├── README.md
└── deet/
    ├── Cargo.toml
    ├── Makefile
    └── src/
```

## Requirements

- Linux on `x86_64`
- Rust toolchain with `cargo`
- Target binaries compiled with DWARF debug info, preferably with low optimization

Recommended flags for target programs:

```bash
-g -O0 -fno-omit-frame-pointer -no-pie
```

## Build

Build the debugger:

```bash
cd deet
cargo build
```

Build an optimized release binary:

```bash
cd deet
cargo build --release
```

The release executable will be generated at:

```bash
deet/target/release/deet
```

## Run

Start the debugger with a target program path:

```bash
cd deet
cargo run -- <target-program>
```

Or run the release binary directly:

```bash
./target/release/deet <target-program>
```

Example:

```bash
./target/release/deet ./my_program
```

Note that the current CLI accepts exactly one positional argument: the target program path.

## Supported Commands

Inside the debugger, the following commands are currently implemented:

- `run`
- `continue`
- `stepi`
- `break <*addr|line|function>`
- `delete <breakpoint-number|*addr|line|function>`
- `backtrace`
- `print <$reg|0xaddr|variable>`
- `x <*addr|line|function>`
- `info breakpoints`
- `info registers`
- `info locals`
- `jump <*addr|line|function>`
- `help`
- `quit`

## Breakpoint Syntax

`break` and related location-based commands accept these forms:

- `break *0x401136`
- `break 12`
- `break main`

In practice, function names and raw addresses are the most reliable forms. Line-based resolution depends on the quality of the target's DWARF line table.

## Printing Values

`print` currently supports:

- Registers, for example `print $rip`
- Raw addresses, for example `print 0x404020`
- Known variable names, for example `print a`

Variable printing uses DWARF location information and currently handles common cases such as:

- Global variables
- Stack locals addressed relative to the frame pointer
- Parameters passed in common `x86_64` System V argument registers
- Variables with basic location-list coverage selected by the current program counter

## Current Language Support

`deet` is not tied to a specific source language. It works with native binaries that match the current implementation assumptions:

- Linux `x86_64`
- DWARF debug information available
- Calling convention compatible with the implemented register and stack logic

In practice, the most realistic supported languages are:

- C
- C++
- Rust

As long as those programs are compiled to native Linux binaries with usable DWARF data, the debugger can usually resolve functions, lines, and some variables.

## Known Limitations

This project is still intentionally small in scope. Important limitations include:

- Only Linux `x86_64` is assumed
- The debugger relies on `ptrace`
- The CLI currently accepts only a target program path, not target arguments
- Variable recovery is incomplete for highly optimized code
- Complex DWARF expressions are not fully supported
- Some line-based breakpoint resolutions may be inaccurate
- Entry-point variable values can be transient before stack-frame setup is fully stable
- Support is strongest for integer and pointer-like values, not arbitrary complex types

For best results, debug binaries should be compiled with:

```bash
-g -O0 -fno-omit-frame-pointer -no-pie
```

## Example Session

```text
(deet) break main
(deet) run
(deet) info breakpoints
(deet) info locals
(deet) print $rip
(deet) stepi
(deet) backtrace
(deet) continue
(deet) quit
```

## Creating a Downloadable Release

To produce a release build for distribution:

```bash
cd deet
cargo build --release
```

Then package the binary:

```bash
mkdir -p dist
cp target/release/deet dist/
tar -czf deet-linux-x86_64.tar.gz -C dist .
```

This produces a Linux `x86_64` release artifact that can be shared with other users on compatible systems.

## Development Notes

Core implementation files are in `deet/src/`:

- `debugger.rs`: command dispatch and user-facing debugger behavior
- `inferior.rs`: traced child process control
- `dwarf_data.rs`: higher-level DWARF-backed symbol and variable lookup
- `gimli_wrapper.rs`: lower-level DWARF parsing with `gimli`
- `debugger_command.rs`: command parsing

## License

No license file is currently included in this repository.
