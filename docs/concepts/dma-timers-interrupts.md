# DMA, Timers, and Interrupts

The PS1 has seven DMA channels:

| Channel | Use |
|---|---|
| 0 | MDEC input |
| 1 | MDEC output |
| 2 | GPU packets and image data |
| 3 | CD-ROM to RAM |
| 4 | SPU transfers |
| 5 | Expansion port |
| 6 | Ordering table clear |

The interrupt controller exposes `I_STAT` and `I_MASK`. An interrupt reaches
the CPU when `I_STAT & I_MASK` is non-zero and COP0 status enables the hardware
interrupt line.

Root counters start with system-clock behavior and later need dotclock,
HBlank, VBlank, target, overflow, pulse/toggle, and one-shot/repeat accuracy.

