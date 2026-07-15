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
  GPU linked-list DMA, OTC, and central device MMIO integration
- GPU GP0/GP1 command parsing, VRAM upload/readback/copy, flat and textured
  primitive rendering, display register state, and BGR555 frame extraction
- CD-ROM command/status basics with cooked ISO/raw BIN/single-track CUE sector
  delivery and DMA3 transfer support
- GTE register model with documented geometry, arithmetic, lighting, color,
  interpolation, and depth-cue commands, UNR projection division, and CPU
  command interlocks
- SIO0/JOY controller polling for digital and analog pads, delayed IRQ7, and
  persistent raw memory-card images with sector read/write protocols
- SPU register/RAM model with DMA4, IRQ9, 24 SPU-ADPCM voices, ADSR, pitch and
  noise modes, capture buffers, reverb routing, and 44.1 kHz stereo output
- Minimal PS-X EXE loader and CLI runner with BIOS boot trace output
- Focused unit tests for CPU, memory, graphics, I/O, and audio behavior

It is not yet a playable emulator. Phase 1 CPU/bus, Phase 2 BIOS boot bring-up,
Phase 3 DMA/timer/IRQ behavior, and the Phase 4 core GPU path are complete
enough to move on. Phase 5 CD-ROM now has BIOS-facing command coverage and
single data-track image mounting in place. Phase 6 GTE now has a documented
command baseline, the hardware projection divider, and command busy timing;
saturation corner cases and per-register pipeline hazards remain accuracy work.
Phase 7 controller and memory-card baselines and the Phase 8 SPU baseline are
complete. The next milestones are MDEC, GPU timing accuracy, full CD-ROM
image/timing/audio compatibility, save states, and an SDL frontend.

## Run

```sh
cargo test

cargo run -p ps1-frontend -- --bios path/to/SCPH1001.BIN --steps 100000
cargo run -p ps1-frontend -- --bios path/to/SCPH1001.BIN --steps 100000 --trace debug/boot.trace
cargo run -p ps1-frontend -- --bios path/to/SCPH1001.BIN --exe path/to/demo.exe --steps 100000
cargo run -p ps1-frontend -- --bios path/to/SCPH1001.BIN --disc path/to/game.iso --steps 100000
cargo run -p ps1-frontend -- --bios path/to/SCPH1001.BIN --memory-card saves/card1.mcd --steps 100000
cargo run -p ps1-frontend -- --bios path/to/SCPH1001.BIN --steps 1000000 --audio-dump debug/spu.wav
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

`--memory-card PATH` and `--memory-card2 PATH` mount raw 128 KiB card images in
slots 1 and 2. A missing path starts as a formatted card and is created when the
run ends; existing raw images are updated with writes made by emulated software.

`--audio-dump PATH` streams the SPU's native 44.1 kHz stereo signed-16 output to
a WAV file. The core API exposes the same interleaved samples through
`Ps1::drain_audio` for a later realtime frontend.

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
- PSX-SPX GTE: https://psx-spx.consoledev.net/geometrytransformationenginegte/
- PSX-SPX SPU: https://psx-spx.consoledev.net/soundprocessingunitspu/
- PSX-SPX controllers and memory cards: https://psx-spx.consoledev.net/controllersandmemorycards/
- PSX-SPX serial interfaces: https://psx-spx.consoledev.net/serialinterfacessio/
- PSX-SPX DMA: https://psx-spx.consoledev.net/dmachannels/
- PSX-SPX timers: https://psx-spx.consoledev.net/timers/
- PSX-SPX interrupts: https://psx-spx.consoledev.net/interrupts/
