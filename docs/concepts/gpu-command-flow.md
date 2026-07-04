# GPU Command Flow

The PS1 GPU is packet-driven.

1. CPU writes GP0/GP1 registers directly, or DMA2 streams packets to GP0.
2. GP0 commands draw into 1024x512 16-bit VRAM or transfer image data.
3. GP1 display registers choose which VRAM rectangle appears on screen.

There is no depth buffer. Games sort primitives into ordering tables in main
RAM, then send them with DMA2 linked-list mode. DMA6 initializes empty ordering
tables by writing reverse links.

