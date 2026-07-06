# Memory Map

The CPU sees KUSEG, KSEG0, and KSEG1 as mirrors of low physical memory.

Important regions:

| Physical range | Device |
|---|---|
| `0x00000000..0x001fffff` | 2 MB main RAM |
| `0x1f800000..0x1f8003ff` | 1 KB scratchpad |
| `0x1f801000..0x1f801fff` | I/O registers |
| `0x1fc00000..0x1fc7ffff` | 512 KB BIOS |

GPU VRAM and SPU RAM are not directly CPU-addressable. Software reaches them
through MMIO registers and DMA.

Cached KUSEG/KSEG0 instruction fetches go through the i-cache model. KSEG1
instruction fetches bypass it, which is the path used by the reset vector at
`0xbfc00000`. Instruction fetches from scratchpad, MMIO, KSEG2, or unmapped
physical space raise instruction bus errors. Data accesses to unmapped space
raise data bus errors.
