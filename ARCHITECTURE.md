# PS1 Emulator - Technical Architecture

## High-Level Shape

```text
+--------------------------------------------------------+
|                         Ps1                            |
|                                                        |
|  +-------------+     borrows      +------------------+ |
|  | R3000A CPU  | ---------------> |       Bus        | |
|  | COP0 + GTE  |                  | RAM / BIOS / MMIO| |
|  +-------------+                  +------------------+ |
|                                           |            |
|    +-------------+  +-----------+  +------+-------+    |
|    | Scheduler   |  | Timers    |  | Interrupts  |    |
|    +-------------+  +-----------+  +--------------+    |
|                                                        |
|    +-------------+  +-----------+  +--------------+    |
|    | DMA         |  | GPU/VRAM  |  | SPU/Sound RAM|    |
|    +-------------+  +-----------+  +--------------+    |
|                                                        |
|    +-------------+  +-----------+  +--------------+    |
|    | CD-ROM      |  | MDEC      |  | JOY/MemoryCard|   |
|    +-------------+  +-----------+  +--------------+    |
+--------------------------------------------------------+
```

Like the GBA project, the CPU and bus are sibling fields. `Ps1::step_one()`
borrows them independently:

```rust
let cycles = self.cpu.step(&mut self.bus);
```

Cross-device effects stay inside `Bus` where possible. For example, writing DMA
channel control can immediately run the transfer because `Bus` owns RAM, DMA,
GPU, SPU, CD-ROM, and interrupts.

## Memory Map

The CPU virtual segments mirror physical memory:

| Virtual range | Cached | Physical mapping |
|---|---:|---|
| `0x00000000..0x1fffffff` KUSEG | yes | direct low physical mirror |
| `0x80000000..0x9fffffff` KSEG0 | yes | subtract `0x80000000` |
| `0xa0000000..0xbfffffff` KSEG1 | no | subtract `0xa0000000` |
| `0xc0000000..0xffffffff` KSEG2 | no | kernel/control space |

Important physical regions:

| Physical range | Size | Device |
|---|---:|---|
| `0x00000000..0x001fffff` | 2 MB | Main RAM, mirrored through first 8 MB |
| `0x1f000000..0x1f7fffff` | up to 8 MB | Expansion region 1 |
| `0x1f800000..0x1f8003ff` | 1 KB | Scratchpad |
| `0x1f801000..0x1f801fff` | 4 KB | I/O registers |
| `0x1f802000..0x1f803fff` | 8 KB | Expansion region 2 |
| `0x1fc00000..0x1fc7ffff` | 512 KB | BIOS ROM |

The GPU VRAM and SPU RAM are not directly CPU-mapped. They are accessed through
GPU/SPU registers and DMA.

## CPU

The first CPU target is a MIPS R3000A-style interpreter:

- 32 general registers, `hi`, `lo`, `pc`, `next_pc`
- Branch delay via `pc`/`next_pc`
- One-instruction load delay
- COP0 status/cause/EPC/basic exception handling
- COP2/GTE register-transfer scaffold

The implementation starts with a simple fixed cycle cost per instruction. Bus
wait states, DMA contention, cache behavior, and GTE pipeline timings belong in
later accuracy phases.

## MMIO Ownership

`Bus` owns all memory-visible devices:

- `InterruptController` for `I_STAT` and `I_MASK`
- `DmaController` for channel registers, `DPCR`, and `DICR`
- `Timers` for root counters
- `Gpu` for `GP0`, `GP1`, `GPUREAD`, and `GPUSTAT`
- `Spu` for voice/control registers and 512 KB sound RAM
- `Cdrom` for command/response/data register scaffolding
- `Mdec` for command/status scaffolding

This is intentionally similar to the GBA bus design: MMIO writes decode at one
central boundary, then mutate the owned device.

## Rendering Plan

The PS1 GPU receives packets, not tiles:

1. CPU or DMA writes GP0 command packets.
2. GPU rasterizes commands into 1024x512 16-bit VRAM.
3. GP1 display registers select a rectangle inside VRAM for video output.

There is no hardware depth buffer. Games sort primitives in RAM ordering tables,
then send those tables through GPU linked-list DMA. That makes DMA2 and DMA6
early priorities.

## Source Notes

The hardware details in this plan are based mainly on PSX-SPX:

- Memory map and cache/scratchpad behavior: https://psx-spx.consoledev.net/memorymap/
- I/O register ranges: https://psx-spx.consoledev.net/iomap/
- CPU opcode map: https://psx-spx.consoledev.net/cpuspecifications/
- GPU commands and VRAM: https://psx-spx.consoledev.net/graphicsprocessingunitgpu/
- DMA channels: https://psx-spx.consoledev.net/dmachannels/
- Root counters: https://psx-spx.consoledev.net/timers/
- Interrupt controller: https://psx-spx.consoledev.net/interrupts/

