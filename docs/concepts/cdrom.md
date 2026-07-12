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
- `Pause`
- `GetlocL`

The current implementation provides an immediate deterministic baseline:

- Indexed status/command/parameter/response/data registers
- Interrupt enable/flag handling for ACK, complete, data-ready, and error
  responses
- Cooked 2048-byte sector images and raw 2352-byte Mode 2 images
- DMA3 transfer from CD-ROM data FIFO into main RAM
- Frontend mounting through `--disc PATH` and `--disc-sector-size 2048|2352`

Sector timing, CUE/session metadata, repeated read cadence, seek latency, XA
filtering, CD-DA, and copy-protection details belong in later compatibility
phases.
