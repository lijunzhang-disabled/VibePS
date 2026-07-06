# Phase 3 DMA/Timer/IRQ Pass

## Scope

- Added DMA controller start semantics for sync modes 0, 1, and 2.
- Modeled DPCR priority/enables and DICR masked completion flags, bus-error
  master flag behavior, channel flag acknowledges, and IRQ3 edge requests.
- Enforced DMA6/OTC CHCR restrictions.
- Routed pending DMA through bus priority selection.
- Added MDEC, GPU, CD-ROM, SPU, and OTC DMA channel paths using deterministic
  immediate transfers.
- Added root-counter target/overflow flags, reset-on-target,
  one-shot/repeat, pulse/toggle IRQ behavior, timer2 `sysclk/8`, HBlank clock
  hooks, and HBlank/VBlank synchronization state.

## Deferred Accuracy

- DMA bus stealing, CPU stall windows, GPU FIFO back-pressure, and exact
  transfer-cycle accounting.
- GPU-derived dotclock and real HBlank cadence.
- Full MDEC/CD-ROM/SPU device timing beyond their current transfer surfaces.

## Regression

- DMA2 linked-list GP0 packets.
- DMA2 sync1 VRAM upload MADR/BCR completion.
- DMA6 OTC ordering-table clear and IRQ3 request.
- DICR completion/bus-error/master flag behavior.
- Timer one-shot/repeat, pulse/toggle, HBlank clock, sync pause, and
  `sysclk/8` behavior.
