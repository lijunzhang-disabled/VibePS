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
- `GetlocP`
- `GetTN` / `GetTD`
- `SeekL` / `SeekP`
- `GetID`

The current implementation provides an immediate deterministic baseline:

- Indexed status/command/parameter/response/data registers
- Interrupt enable/flag handling for ACK, complete, data-ready, and error
  responses
- Deterministic `MotorOn`, `Stop`, `Mute`, `Demute`, `Setfilter`, `GetTN`,
  `GetTD`, `Seek`, `GetID`, and `ReadTOC` command responses
- Cooked 2048-byte sector images and raw 2352-byte Mode 2 images
- Single data-track CUE mounting with FILE/TRACK/INDEX 01 parsing
- DMA3 transfer from CD-ROM data FIFO into main RAM
- Frontend mounting through `--disc PATH` and `--disc-sector-size 2048|2352`

Sector timing, multi-track/session metadata, repeated read cadence, seek
latency, XA filtering, CD-DA, and copy-protection details belong in later
compatibility phases.
