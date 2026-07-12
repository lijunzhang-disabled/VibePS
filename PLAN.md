# PS1 Emulator - Implementation Plan

## Current Status

| Phase | Status | Scope |
|---|---|---|
| Phase 0: Research + skeleton | Done | Workspace, architecture docs, core crate, CLI harness |
| Phase 1: CPU + memory bus | Done | R3000A integer core, COP0, load/branch delays, memory map, bus faults, tests |
| Phase 2: BIOS boot | Done | BIOS boot trace CLI, BCC/cache isolation, i-cache fetch model, PS-X EXE loader |
| Phase 3: DMA + timers + IRQs | Done | DMA modes/channels, GPU linked lists, OTC, DICR/DPCR IRQs, root-counter modes |
| Phase 4: GPU | Done | GP0/GP1 parser, VRAM transfers, polygons, rectangles, display output |
| Phase 5: CD-ROM | In progress | Command/status basics, Setloc/ReadN sector reads, cooked ISO/raw BIN data, DMA3 |
| Phase 6: GTE | Pending | COP2 register model and matrix/vector commands |
| Phase 7: Controllers + memory cards | Pending | JOY serial protocol, digital/analog pads, card EEPROM protocol |
| Phase 8: SPU | Pending | 24 ADPCM voices, ADSR, pitch, reverb, CD audio mixing |
| Phase 9: MDEC + compatibility | Pending | FMV decode path, game-focused bug fixing, save states |

## Hardware Targets

| Component | First target |
|---|---|
| CPU | MIPS R3000A user/kernel core, branch delay, load delay, COP0 exceptions |
| Memory | 2 MB RAM mirror, 1 KB scratchpad, 512 KB BIOS, I/O map dispatch |
| GPU | GP0/GP1 command FIFO and CPU/DMA VRAM upload first, polygons after |
| DMA | Channels 2 and 6 first because GPU linked lists and OTC are core boot/game paths |
| Timers | System-clock timers first, then dotclock/HBlank/VBlank sync |
| CD-ROM | BIOS command compatibility before XA/audio details |
| GTE | Register transfers first, RTPS/RTPT/NCLIP/AVSZ/MVMVA next |
| SPU | RAM transfer path first, then ADPCM voice output |

## Phase 1 Details: CPU + Bus

1. Implement all documented R3000A integer opcodes. Done for the integer core; COP2/GTE math remains Phase 6.
2. Add tests for ALU, branches, load delays, unaligned loads/stores, COP0, and exceptions. Done with native Rust regressions and imported PCSX-Redux CPU cases.
3. Model KUSEG/KSEG0/KSEG1 physical mirrors. Done for RAM, scratchpad, MMIO, expansion windows, BIOS, and cache-control space.
4. Add misalignment and bus-error exceptions. Done for AdEL/AdES plus IBE/DBE on non-executable or unmapped accesses.
5. Add instruction tracing compatible with the style used in `../gba` and `../nds`. Done through the frontend `--trace` harness.

## Phase 2 Details: BIOS Boot

1. Require a real BIOS first; BIOS HLE can wait. Done: the harness accepts a user-provided BIOS image and does not implement BIOS HLE.
2. Boot from `0xBFC00000`. Done in CPU reset and `Ps1::new`.
3. Record a boot trace with `ps1-frontend --trace` and compare against known-good emulator traces. Done for trace generation; external comparison depends on a user-provided BIOS/reference trace.
4. Implement i-cache and scratchpad/cache-control behavior only when tests or BIOS traces need it. Done for BCC, isolated cache data accesses, per-word i-cache valid bits, cached KUSEG/KSEG0 fetches, and uncached KSEG1 fetches. Cycle timing remains a later accuracy pass.
5. Keep direct PS-X EXE loading for CPU and demo bring-up. Done with success and error-path tests.

## Phase 3 Details: DMA + Timers + IRQs

1. Finish DMA channel modes 0, 1, and 2. Done for deterministic immediate transfers, including MADR/BCR completion state where applicable.
2. Implement GPU linked-list DMA and OTC precisely enough for ordering tables. Done with DMA2 linked-list GP0 packets and DMA6 reverse ordering-table clear tests.
3. Implement DICR/DPCR interrupt behavior. Done for channel priority/enable, forced burst start, masked completion flags, bus-error/master flags, and IRQ3 master-flag edges.
4. Finish root-counter target, overflow, one-shot/repeat, pulse/toggle, dotclock, HBlank, and VBlank modes. Done with target/overflow flags, IRQ modes, timer2 divide-by-8, HBlank clock hooks, sync pause/reset/free-run state, and VBlank scheduler edges. Exact GPU-derived dotclock/HBlank timing remains a later accuracy pass.

## Phase 4 Details: GPU

1. Parse GP0 packets and GP1 display-control commands. Done with a shared direct-CPU/DMA packet collector, GP1 display state, GPUSTAT status bits, and internal register reads.
2. Implement VRAM CPU-to-GPU and GPU-to-CPU transfers. Done for CPU/DMA upload, readback through GPUREAD, and VRAM-to-VRAM copies with wrapping and mask behavior.
3. Implement rectangle and monochrome polygon rendering. Done for flat rectangles, lines, triangles, and quads.
4. Add texture, CLUT, Gouraud, semi-transparency, dithering, masking, and draw-area rules. Done as a deterministic software-rendering baseline for 4bpp/8bpp/15bpp texture lookup, CLUTs, modulation/raw texture mode, Gouraud interpolation, blend modes, dithered 15-bit conversion, draw offset/area clipping, and mask-bit rules.
5. Add an SDL frontend once the display path shows BIOS/demo output. Deferred until a real BIOS/demo reaches stable visible frames; the core now exposes `display_frame()` and display sizing for that frontend.

## Phase 5 Details: CD-ROM

1. Implement command/status register basics. In progress: indexed register
   access, response/data FIFOs, interrupt enable/flag handling, `Nop`, `Init`,
   `Setmode`, `Getparam`, `Setloc`, `ReadN`, `ReadS`, `Pause`, and `GetlocL`
   are implemented as an immediate deterministic baseline.
2. Provide sector data from mounted images. In progress: cooked 2048-byte
   images and raw 2352-byte images are supported, with raw Mode 2 payloads read
   from offset 24.
3. Move sectors through DMA3. Done for the immediate DMA path, covered by a
   bus-level test.
4. Add BIN/CUE track/session parsing, response timing, repeated read cadence,
   seek latency, error details, and XA/CD-DA behavior. Pending.

## Phase 5+ Compatibility

The PS1 has several accuracy traps that should be handled incrementally:

- R3000A load delays and branch delay exception EPC/BD behavior
- COP0 interrupt level behavior tied to `I_STAT & I_MASK`
- DMA bus stealing and GPU FIFO back-pressure
- Exact GPU-derived dotclock/HBlank timing and DMA CPU stall windows
- GPU has no depth buffer; ordering table behavior matters
- CD-ROM response/data queues and command latency
- SPU transfer timing and delayed/unstable reads
- GTE saturation flags and pipeline timings
