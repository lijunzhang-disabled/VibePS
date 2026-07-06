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

The DMA controller models channel priorities/enables, forced burst starts,
sync modes 0/1/2, DICR masked completion flags, bus-error/master IRQ flags, and
IRQ3 edges. DMA2 linked-list mode feeds GPU GP0 packets, DMA2 sync1 can upload
VRAM data, DMA3 reads CD-ROM data, DMA4 transfers SPU data, and DMA6 clears
ordering tables.

Root counters model target/overflow flags, reset-on-target, one-shot/repeat,
pulse/toggle IRQ behavior, timer2 `sysclk/8`, HBlank clock edges, and
HBlank/VBlank synchronization state. Exact GPU-derived dotclock and HBlank
timing are still later accuracy work; the current hooks give deterministic
behavior for core and BIOS bring-up tests.
