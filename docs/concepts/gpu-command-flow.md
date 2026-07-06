# GPU Command Flow

The PS1 GPU is packet-driven.

1. CPU writes GP0/GP1 registers directly, or DMA2 streams packets to GP0.
2. GP0 commands draw into 1024x512 16-bit VRAM or transfer image data.
3. GP1 display registers choose which VRAM rectangle appears on screen.

There is no depth buffer. Games sort primitives into ordering tables in main
RAM, then send them with DMA2 linked-list mode. DMA6 initializes empty ordering
tables by writing reverse links.

## Current Implementation

`ps1-core::gpu::Gpu` keeps GP0 input state so direct CPU writes and DMA2 writes
use the same parser. Implemented GP0 behavior includes:

- quick fill, CPU-to-VRAM, VRAM-to-CPU, and VRAM-to-VRAM transfers;
- flat rectangles, lines, triangles, and quads;
- textured rectangles/polygons with 4bpp, 8bpp, and 15bpp lookup, CLUTs,
  raw/modulated texture mode, texture windows, and rectangle flip bits;
- Gouraud color interpolation, semi-transparent blend modes, dithered 15-bit
  conversion, draw offset, draw-area clipping, and mask-bit writes/checks.

GP1 tracks reset, command-buffer reset, IRQ acknowledge, display enable, DMA
direction, display start/range/mode, and internal register reads. `display_frame`
returns the selected BGR555 display rectangle.

## Accuracy Still Deferred

The renderer is deterministic and good enough for bring-up, but not yet a
cycle-accurate GPU. FIFO depth, command execution timing, DMA back-pressure,
exact raster edge rules, interlaced field behavior, 24-bit display output, and
old-GPU revision quirks remain compatibility work.
