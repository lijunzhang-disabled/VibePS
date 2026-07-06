# Phase 1/2 Completion Pass

## Scope

- Closed the Phase 1 CPU/bus gaps by adding instruction bus error and data bus
  error paths to the interpreter.
- Routed instruction fetches through `Bus::fetch32`, so cached KUSEG/KSEG0
  fetches use the i-cache model while KSEG1 fetches bypass it.
- Added per-word i-cache valid-bit behavior on refill.
- Kept exact cache refill timing and write queue timing out of scope for this
  milestone; those belong in later accuracy work.

## Verification Added

- Scratchpad instruction fetch raises IBE.
- Unmapped data load raises DBE.
- KSEG0 fetch sees a cached line even after RAM is modified.
- KSEG1 fetch bypasses i-cache and sees modified RAM.

## Result

`PLAN.md` now marks Phase 1 and Phase 2 done. Remaining work starts at Phase 3:
DMA, timers, and interrupt accuracy.
