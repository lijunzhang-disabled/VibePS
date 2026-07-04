# CD-ROM

The CD-ROM controller is command and FIFO based. Software writes a command and
parameters through indexed registers, then receives status bytes, interrupt
responses, and sector data.

The first target is BIOS command compatibility:

- `Nop`
- `Init`
- `Setmode`
- `Getparam`
- `Setloc`
- `ReadN` / `ReadS`

Sector timing, XA filtering, CD-DA, and copy-protection details belong in later
compatibility phases.

