# Online CPU Test Survey

## Source Checked

- PCSX-Redux test programs: https://github.com/grumpycoders/pcsx-redux/tree/main/src/mips/tests
- PCSX-Redux CPU tests: https://github.com/grumpycoders/pcsx-redux/tree/main/src/mips/tests/cpu
- PCSX-Redux runner references `cpu.ps-exe`, `cop0.ps-exe`, and OpenBIOS paths, but the shallow clone did not contain prebuilt `*.ps-exe` or `openbios.bin` artifacts.

## Local Build Attempt

```sh
make -C /private/tmp/pcsx-redux/src/mips/tests/cpu
```

Result: blocked because `mipsel-none-elf-gcc` / `mipsel-none-elf-g++` are not installed in this environment.

## Imported Into Core Tests

Low-level PCSX-Redux CPU cases were ported into native Rust unit tests:

- A normal write to the same register cancels a pending delayed load.
- A `jal` link write to `$ra` cancels a pending delayed load to `$ra`.
- Consecutive loads to the same register keep the first delayed load invisible.
- `LWL/LWR` no-delay and delayed merge cases from `lwlr.s`.
- `LW/LWR` merge behavior when the base value comes from a pending `LW`.
- Signed/unsigned divide-by-zero HI/LO behavior.
- `BLTZAL` writes `$ra` even when the branch is not taken.
- Branch/jump in branch/jump delay-slot execution order from `branchbranch.s`.

These exposed and fixed real issues in the interpreter:

- Delayed loads were always committed after the following instruction, even when that instruction wrote the same target register.
- Consecutive loads to the same register committed the first load too early instead of keeping it invisible.
- Relative branches executed in a branch delay slot used the original branch PC as their base instead of the current PC observed by the R3000A pipeline.
