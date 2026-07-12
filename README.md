# PS1 Emulator in Rust

A PlayStation 1 simulator/emulator project, structured to match the sibling
`../gba` and `../nds` projects: a platform-independent core crate plus a small
frontend crate.

This repository currently contains the first implementation slice:

- Rust workspace with `ps1-core` and `ps1-frontend`
- MIPS R3000A CPU foundation with branch/load delay behavior
- PS1 virtual-to-physical memory map, RAM, scratchpad, BIOS ROM, and MMIO
- BIU/cache-control register, isolated-cache data access behavior, cached
  KUSEG/KSEG0 instruction fetches, and uncached KSEG1 fetches
- Interrupt controller, root-counter timer modes, DMA channel modes/IRQs,
  GPU linked-list DMA, OTC, and GPU/SPU/CD scaffolding
- GPU GP0/GP1 command parsing, VRAM upload/readback/copy, flat and textured
  primitive rendering, display register state, and BGR555 frame extraction
- CD-ROM command/status basics with cooked ISO/raw BIN/single-track CUE sector
  delivery and DMA3 transfer support
- GTE register model with first geometry/depth commands: `RTPS`, `RTPT`,
  `NCLIP`, `AVSZ3`, `AVSZ4`, and `MVMVA`
- Minimal PS-X EXE loader and CLI runner with BIOS boot trace output
- Focused unit tests for CPU and memory behavior

It is not yet a playable emulator. Phase 1 CPU/bus, Phase 2 BIOS boot bring-up,
Phase 3 DMA/timer/IRQ behavior, and the Phase 4 core GPU path are complete
enough to move on. Phase 5 CD-ROM now has BIOS-facing command coverage and
single data-track image mounting in place. Phase 6 GTE has its first
geometry/depth slice. The next milestones are fuller GTE lighting/color
coverage, SPU, controllers, memory cards, GPU timing accuracy, full CD-ROM
image/timing compatibility, and an SDL frontend.

## Run

```sh
cargo test

cargo run -p ps1-frontend -- --bios path/to/SCPH1001.BIN --steps 100000
cargo run -p ps1-frontend -- --bios path/to/SCPH1001.BIN --steps 100000 --trace debug/boot.trace
cargo run -p ps1-frontend -- --bios path/to/SCPH1001.BIN --exe path/to/demo.exe --steps 100000
cargo run -p ps1-frontend -- --bios path/to/SCPH1001.BIN --disc path/to/game.iso --steps 100000
cargo run -p ps1-frontend -- --exe path/to/test.ps-exe --test-mailbox 0x80010100=1 --steps 100000
```

The frontend is currently a bring-up harness. It prints final CPU/video/audio
state rather than opening a video/audio window. `--trace` writes one
pre-instruction CPU state line per executed instruction so BIOS boot can be
diffed against a known-good emulator. The core exposes BGR555 frames through the
GPU display path for the later SDL/video frontend.

`ps1-core::test_runner` provides reusable PS-EXE test execution infrastructure.
The frontend exposes it through `--test`, `--test-mailbox ADDR=PASS`,
`--test-stop-pc ADDR`, `--test-pass-reg REG=VALUE`, and `--test-exit-reg REG`.
A mailbox run stops when the 32-bit mailbox becomes nonzero and passes only when
it equals the requested pass value.

`--disc PATH` mounts a simple cooked 2048-byte/sector image, raw
2352-byte/sector image, or single data-track `.cue` pointing at one of those
images. The sector size is auto-detected from the extension/file length or CUE
track mode, and can be forced with `--disc-sector-size 2048|2352`.

## Project Docs

| Doc | What it is for |
|---|---|
| [`PLAN.md`](PLAN.md) | Phase plan and implementation roadmap |
| [`ARCHITECTURE.md`](ARCHITECTURE.md) | Core design for CPU, bus, MMIO, GPU, audio, and scheduling |
| [`debug/`](debug/) | Compatibility investigations and test plans |
| [`docs/concepts/`](docs/concepts/) | Short subsystem notes, matching the sibling NDS project style |

## Hardware Summary

The PS1 is built around a 33.8688 MHz MIPS R3000A-class CPU with COP0, a COP2
Geometry Transformation Engine, 2 MB main RAM, 1 KB scratchpad, 512 KB BIOS ROM,
1 MB GPU VRAM, 512 KB SPU RAM, DMA, root counters, CD-ROM, controllers, and
memory cards.

The CPU sees three important virtual mirrors of the same low physical space:
`KUSEG`, cached `KSEG0`, and uncached `KSEG1`. The boot vector starts in the
BIOS mirror at `0xBFC00000`.

## References

- PSX-SPX by nocash and contributors: https://psx-spx.consoledev.net/
- PSX-SPX memory map: https://psx-spx.consoledev.net/memorymap/
- PSX-SPX I/O map: https://psx-spx.consoledev.net/iomap/
- PSX-SPX CPU specifications: https://psx-spx.consoledev.net/cpuspecifications/
- PSX-SPX GPU: https://psx-spx.consoledev.net/graphicsprocessingunitgpu/
- PSX-SPX DMA: https://psx-spx.consoledev.net/dmachannels/
- PSX-SPX timers: https://psx-spx.consoledev.net/timers/
- PSX-SPX interrupts: https://psx-spx.consoledev.net/interrupts/
