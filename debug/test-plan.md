# PS1 Test Plan

## Immediate

- Unit-test every R3000A integer opcode class.
- Add focused tests for branch-delay EPC/BD behavior.
- Add unaligned `LWL/LWR/SWL/SWR` tests.
- Continue porting low-level PCSX-Redux CPU/COP0 tests into native core tests until a MIPS PS-EXE toolchain is available.
- Add DMA2 GPU packet and DMA6 OTC tests.

## Bring-Up

- Run a real BIOS for a fixed step count with `--trace` and compare CPU traces against a known-good emulator.
- Run PS-X EXE homebrew that writes known values to RAM/MMIO.
- Add screenshot capture once GPU display output is wired to the frontend.

## Compatibility

- Track each game or test ROM issue as a dated note in `debug/`.
- Every fix should include a focused unit test or a reproducible harness command.
