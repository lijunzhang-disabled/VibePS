# Cache Control Isolation

## Scope

- Added the BIU/cache-control register at `0xFFFE0130`.
- Added a minimal isolated 4KB I-cache backing store for `SR.IsC` data accesses.
- Implemented TAG+IS1 tag writes/reads and IS1 code-word writes/reads well enough for BIOS-style cache flush loops.
- Routed CPU data loads/stores, including unaligned memory opcodes and `LWC2/SWC2`, through isolated cache mode when COP0 `SR.IsC` is set.

## Not Done

- Normal instruction fetches still read memory directly.
- I-cache refill, per-word fetch valid bits, and timing are not modeled yet.
- Write queue timing is still pending.

## Verification

Covered by focused unit tests for:

- BCC register round trip.
- Isolated tag/code writes not touching RAM.
- CPU `SR.IsC` redirecting a data store away from RAM.
