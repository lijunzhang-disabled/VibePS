# 2026-07-04_cpu-delay-exception-fixes

## Symptom

Phase 1 CPU bring-up needed tighter R3000A edge-case behavior before BIOS trace
work. The initial interpreter handled basic branch and load delay cases, but it
had gaps around exceptions in delay slots and unaligned transfer pairs.

## Hypothesis

BIOS and early game code will rely on exact COP0 exception bookkeeping:

- conditional branches always have a delay slot, even when not taken
- exceptions in a delay slot must set Cause.BD and EPC to the branch address
- address exceptions must update BadVAddr
- `LWL/LWR` pairs need to merge through the pending load value
- `SWR` must write the low-order bytes for little-endian unaligned stores

## Evidence

Added focused tests for:

- taken branch delay-slot address exception
- not-taken branch delay-slot address exception
- misaligned instruction fetch
- `SWL/SWR` plus `LWL/LWR` unaligned round-trip

The unaligned round-trip initially failed with the high byte missing, confirming
that `LWR` was not seeing the pending `LWL` result.

## Fix

- Mark conditional branch delay slots regardless of branch outcome.
- Pass the current instruction PC into exception paths instead of deriving it
  from the already-advanced `self.pc`.
- Add COP0 BadVAddr updates and Cause coprocessor bits.
- Keep a transient pending-load merge value so adjacent `LWL/LWR` pairs work
  while normal load-delay behavior remains unchanged.
- Correct little-endian `SWR` byte ordering.
- Add alignment checks for instruction fetch and COP2 word loads/stores.

## Regression

`cargo test` now runs 10 passing core tests.

