# BIOS Trace Runner

## Scope

- Added `ps1-frontend --trace PATH`.
- Each line records the pre-instruction PC, next PC, opcode, all general registers, HI/LO, COP0 status/cause/EPC/BadVaddr, and IRQ status/mask.
- Opcode lookup uses `Bus::peek32` so trace formatting does not mutate open-bus state or trigger MMIO reads.

## Current Use

```sh
cargo run -p ps1-frontend -- --bios path/to/SCPH1001.BIN --steps 100000 --trace debug/boot.trace
```

## Next Checks

- Compare early BIOS traces against a known-good interpreter.
- Implement i-cache/cache-control and write queue behavior when the trace shows the BIOS depending on it.
- Keep CPU exception fixes covered by focused unit tests before widening into GPU/DMA boot behavior.
