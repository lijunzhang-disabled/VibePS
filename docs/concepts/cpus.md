# CPUs

The PS1 CPU is a MIPS R3000A-class core running at 33.8688 MHz. It has 32
general registers, `hi`/`lo`, COP0 for exceptions and interrupt control, and
COP2 for the Geometry Transformation Engine.

The first emulator target is instruction correctness:

- Integer ALU and shifts
- Branch and jump delay slots
- One-instruction load delay
- COP0 status/cause/EPC behavior
- GTE register transfers before full GTE math
- `SR.IsC` isolated-cache behavior for BIOS cache flush loops

Cycle accuracy comes later. The initial interpreter uses a fixed instruction
cost so BIOS and test-program control flow can be debugged first.

The BIU/cache-control register at `0xFFFE0130` is modeled as bus state. When
COP0 `SR.IsC` is set, data loads and stores are redirected to a small isolated
I-cache model instead of RAM/MMIO. Instruction-cache fetch timing and refill
accuracy are still future BIOS-trace work.
