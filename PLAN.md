# PS1 Emulator - Implementation Plan

## Current Status

| Phase | Status | Scope |
|---|---|---|
| Phase 0: Research + skeleton | Done | Workspace, architecture docs, core crate, CLI harness |
| Phase 1: CPU + memory bus | In progress | R3000A core, COP0, load/branch delays, memory map, tests |
| Phase 2: BIOS boot | Started | BIOS boot trace CLI, BCC/cache isolation, then i-cache/write queue refinements |
| Phase 3: DMA + timers + IRQs | Started | Real DMA timing, root-counter sync, interrupt edge behavior |
| Phase 4: GPU | Pending | GP0/GP1 parser, VRAM transfers, polygons, rectangles, display output |
| Phase 5: CD-ROM | Pending | Command/status machine, sector reads, ISO/BIN/CUE, XA timing |
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

1. Implement all documented R3000A integer opcodes.
2. Add tests for ALU, branches, load delays, unaligned loads/stores, COP0, and exceptions.
3. Model KUSEG/KSEG0/KSEG1 physical mirrors.
4. Add misalignment and bus-error exceptions.
5. Add instruction tracing compatible with the style used in `../gba` and `../nds`.

## Phase 2 Details: BIOS Boot

1. Require a real BIOS first; BIOS HLE can wait.
2. Boot from `0xBFC00000`.
3. Record a boot trace with `ps1-frontend --trace` and compare against known-good emulator traces.
4. Implement i-cache and scratchpad/cache-control behavior only when tests or BIOS traces need it.
   Current coverage includes the BIU/cache-control register and isolated cache data accesses used by BIOS flush loops.
5. Keep direct PS-X EXE loading for CPU and demo bring-up.

## Phase 3 Details: DMA + Timers + IRQs

1. Finish DMA channel modes 0, 1, and 2.
2. Implement GPU linked-list DMA and OTC precisely enough for ordering tables.
3. Implement DICR/DPCR interrupt behavior.
4. Finish root-counter target, overflow, one-shot/repeat, pulse/toggle, dotclock, HBlank, and VBlank modes.

## Phase 4 Details: GPU

1. Parse GP0 packets and GP1 display-control commands.
2. Implement VRAM CPU-to-GPU and GPU-to-CPU transfers.
3. Implement rectangle and monochrome polygon rendering.
4. Add texture, CLUT, Gouraud, semi-transparency, dithering, masking, and draw-area rules.
5. Add an SDL frontend once the display path shows BIOS/demo output.

## Phase 5+ Compatibility

The PS1 has several accuracy traps that should be handled incrementally:

- R3000A load delays and branch delay exception EPC/BD behavior
- COP0 interrupt level behavior tied to `I_STAT & I_MASK`
- DMA bus stealing and GPU FIFO back-pressure
- GPU has no depth buffer; ordering table behavior matters
- CD-ROM response/data queues and command latency
- SPU transfer timing and delayed/unstable reads
- GTE saturation flags and pipeline timings
