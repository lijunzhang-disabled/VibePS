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

## CPU/BIOS Native Test Closure

Additional local coverage now exercises the CPU/BIOS paths that do not require
a MIPS cross-toolchain or copyrighted BIOS image:

- COP0 `RFE` status-stack restore semantics, matching the PCSX-Redux interpreter formula.
- Exception vector selection for both boot ROM `BEV=1` and RAM `BEV=0` vectors.
- Address-store exceptions from misaligned `SW`, including `BadVaddr`, `EPC`, `BD`, and exception code state.
- Synthetic BIOS execution from `0xbfc00000`.
- CPU reset back to the BIOS boot vector.
- PS-X EXE loading of payload, PC, GP, and SP state.
- PS-X EXE rejection paths for invalid headers, truncated payloads, and payloads crossing the RAM end.

## Remaining External Blockers

- The original PCSX-Redux `cpu.ps-exe` and `cop0.ps-exe` programs still need
  `mipsel-none-elf-gcc` / `mipsel-none-elf-g++` or equivalent prebuilt
  artifacts before they can be run end-to-end.
- Most remaining PCSX-Redux COP0 cases cover debug breakpoint/watchpoint
  behavior through `BPC`, `BDA`, `BDAM`, and `DCIC`. Those should be ported
  after the emulator implements PS1 COP0 debug hardware.
- Real BIOS trace comparison still requires a user-provided BIOS image and a
  known-good reference trace, because the BIOS is not redistributable.
