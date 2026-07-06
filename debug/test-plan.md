# PS1 Test Plan

## Immediate

- Continue porting low-level PCSX-Redux COP0 debug tests after COP0 breakpoint/watchpoint hardware is implemented.

## Completed CPU/BIOS Coverage

- Focused branch-delay EPC/BD/BT exception tests.
- Unaligned `LWL/LWR/SWL/SWR` data-path tests.
- PCSX-Redux CPU regressions for delayed loads, branch-in-delay-slot behavior, jump-in-delay-slot behavior, divide-by-zero, and `BLTZAL` link behavior.
- COP0 interrupt mask, BEV vector, RFE, unusable coprocessor, invalid COP0 read, and misaligned fetch/store exception tests.
- Instruction bus error, data bus error, cached KSEG0 fetch, and uncached KSEG1 fetch tests.
- Synthetic BIOS boot, CPU reset-to-BIOS, and PS-X EXE loader success/error tests.

## Completed DMA/Timer/IRQ Coverage

- DMA2 linked-list GP0 packet transfer.
- DMA2 sync1 VRAM upload with MADR/BCR completion state.
- DMA2 sync1 VRAM readback through GPUREAD into main RAM.
- DMA6 OTC ordering-table clear and IRQ3 request.
- DICR masked completion flags, bus-error master flag, and DMA6 CHCR restrictions.
- DMA pending-channel priority and sync0 force-start behavior.
- Timer target one-shot/repeat, pulse/toggle, timer2 divide-by-8, HBlank clock, and sync pause tests.

## Completed GPU Coverage

- Direct CPU-to-VRAM and VRAM-to-CPU pixel round trip.
- VRAM-to-VRAM copies with mask-bit protection.
- Draw-area clipping and draw offset on flat rectangles.
- Flat triangle rasterization.
- 4bpp textured rectangle sampling through a CLUT.

## Bring-Up

- Run a real BIOS for a fixed step count with `--trace` and compare CPU traces against a known-good emulator.
- Run PS-X EXE homebrew that writes known values to RAM/MMIO once a MIPS PS-EXE toolchain or prebuilt artifact is available.
- Add screenshot capture once a real BIOS/demo produces stable visible frames.

## Compatibility

- Track each game or test ROM issue as a dated note in `debug/`.
- Every fix should include a focused unit test or a reproducible harness command.
