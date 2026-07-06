# PS1 Test Plan

## Immediate

- Continue porting low-level PCSX-Redux COP0 debug tests after COP0 breakpoint/watchpoint hardware is implemented.
- Add DMA2 GPU packet and DMA6 OTC tests.

## Completed CPU/BIOS Coverage

- Focused branch-delay EPC/BD/BT exception tests.
- Unaligned `LWL/LWR/SWL/SWR` data-path tests.
- PCSX-Redux CPU regressions for delayed loads, branch-in-delay-slot behavior, jump-in-delay-slot behavior, divide-by-zero, and `BLTZAL` link behavior.
- COP0 interrupt mask, BEV vector, RFE, unusable coprocessor, invalid COP0 read, and misaligned fetch/store exception tests.
- Instruction bus error, data bus error, cached KSEG0 fetch, and uncached KSEG1 fetch tests.
- Synthetic BIOS boot, CPU reset-to-BIOS, and PS-X EXE loader success/error tests.

## Bring-Up

- Run a real BIOS for a fixed step count with `--trace` and compare CPU traces against a known-good emulator.
- Run PS-X EXE homebrew that writes known values to RAM/MMIO once a MIPS PS-EXE toolchain or prebuilt artifact is available.
- Add screenshot capture once GPU display output is wired to the frontend.

## Compatibility

- Track each game or test ROM issue as a dated note in `debug/`.
- Every fix should include a focused unit test or a reproducible harness command.
