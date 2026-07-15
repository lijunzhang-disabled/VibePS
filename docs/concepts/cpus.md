# CPUs

The PS1 CPU is a MIPS R3000A-class core running at 33.8688 MHz. It has 32
general registers, `hi`/`lo`, COP0 for exceptions and interrupt control, and
COP2 for the Geometry Transformation Engine.

The first emulator target is instruction correctness:

- Integer ALU and shifts
- Branch and jump delay slots
- One-instruction load delay
- COP0 status/cause/EPC behavior
- COP0 interrupt delivery from `SR.IM & Cause.IP`
- GTE register transfers, documented commands, and command busy interlocks
- `SR.IsC` isolated-cache behavior for BIOS cache flush loops
- Instruction bus errors, data bus errors, and cached KUSEG/KSEG0 fetches

The interpreter still uses a fixed base instruction cost. GTE commands are the
first variable timing path: independent CPU work overlaps their documented
latency, while result reads and later commands consume the remaining stall.
General bus and cache cycle accuracy comes later.

The BIU/cache-control register at `0xFFFE0130` is modeled as bus state. When
COP0 `SR.IsC` is set, data loads and stores are redirected to a small isolated
I-cache model instead of RAM/MMIO. Normal instruction fetches use the i-cache
for cached KUSEG/KSEG0 addresses and bypass it for uncached KSEG1 addresses.
The current model tracks tag matches and per-word valid bits; exact refill
timing is still a later accuracy pass.
