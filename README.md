# PS1 Emulator in Rust

A PlayStation 1 simulator/emulator project, structured to match the sibling
`../gba` and `../nds` projects: a platform-independent core crate plus a small
frontend crate.

This repository currently contains the first implementation slice:

- Rust workspace with `ps1-core` and `ps1-frontend`
- MIPS R3000A CPU foundation with branch/load delay behavior
- PS1 virtual-to-physical memory map, RAM, scratchpad, BIOS ROM, and MMIO
- Interrupt controller, root-counter timers, DMA register model, and GPU/SPU/CD
  scaffolding
- Minimal PS-X EXE loader and CLI runner with BIOS boot trace output
- Focused unit tests for CPU and memory behavior

It is not yet a playable emulator. The next milestones are CPU test coverage,
BIOS boot correctness, GPU command rendering, CD-ROM sector delivery, GTE, SPU,
controllers, memory cards, and an SDL frontend.

## Run

```sh
cargo test

cargo run -p ps1-frontend -- --bios path/to/SCPH1001.BIN --steps 100000
cargo run -p ps1-frontend -- --bios path/to/SCPH1001.BIN --steps 100000 --trace debug/boot.trace
cargo run -p ps1-frontend -- --bios path/to/SCPH1001.BIN --exe path/to/demo.exe --steps 100000
```

The frontend is currently a bring-up harness. It prints final CPU state rather
than opening a video/audio window. `--trace` writes one pre-instruction CPU
state line per executed instruction so BIOS boot can be diffed against a
known-good emulator.

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
