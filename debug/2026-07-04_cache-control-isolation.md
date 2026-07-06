# Cache Control Isolation

## Scope

- Added the BIU/cache-control register at `0xFFFE0130`.
- Added a minimal isolated 4KB I-cache backing store for `SR.IsC` data accesses.
- Implemented TAG+IS1 tag writes/reads and IS1 code-word writes/reads well enough for BIOS-style cache flush loops.
- Routed CPU data loads/stores, including unaligned memory opcodes and `LWC2/SWC2`, through isolated cache mode when COP0 `SR.IsC` is set.
- Routed normal instruction fetches through the i-cache for cached KUSEG/KSEG0 addresses.
- Modeled per-word i-cache valid bits and KSEG1 uncached fetch bypass.

## Not Done

- Exact i-cache refill timing is not modeled yet.
- Write queue timing is still pending.

## Verification

Covered by focused unit tests for:

- BCC register round trip.
- Isolated tag/code writes not touching RAM.
- CPU `SR.IsC` redirecting a data store away from RAM.
- KSEG0 fetches using a cached line after RAM is modified.
- KSEG1 fetches bypassing the cached line and seeing modified RAM.
