# Phase 4 GPU Core Pass

## Scope

- Replaced the minimal GP0 special cases with a shared packet collector for CPU
  MMIO writes and DMA2 streams.
- Added GP1 display-control state for display enable, DMA direction, display
  start/range/mode, GPUSTAT bits, and internal register reads.
- Implemented CPU-to-VRAM upload, VRAM-to-CPU readback through GPUREAD, and
  VRAM-to-VRAM copies.
- Added software rendering for flat rectangles, lines, triangles, and quads.
- Added textured rectangle/polygon sampling for 4bpp, 8bpp, and 15bpp
  textures, CLUT lookup, texture windows, raw/modulated texture mode, Gouraud
  interpolation, dithering, semi-transparent blend modes, draw offset/area
  clipping, and mask-bit behavior.
- Kept `display_frame()` as a BGR555 extraction path for the future SDL
  frontend.

## Deferred Accuracy

- GPU FIFO depth, command timing, DMA back-pressure, and CPU stall windows.
- Exact PS1 raster edge rules and old/new GPU revision quirks.
- Interlaced field timing, 24-bit display output, and dotclock/HBlank cadence.
- A windowed SDL frontend and screenshot capture after BIOS/demo visible-frame
  bring-up.

## Regression

- Direct CPU-to-VRAM and VRAM-to-CPU pixel round trip.
- DMA2 sync1 VRAM readback into main RAM.
- VRAM-to-VRAM copy with mask-bit protection.
- Draw-area clipping plus draw offset for rectangles.
- Flat triangle rasterization.
- 4bpp textured rectangle CLUT sampling.
