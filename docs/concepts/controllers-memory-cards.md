# Controllers and Memory Cards

Controllers and memory cards share the SIO0 serial bus. Software selects port 1
or 2 with `JOY_CTRL`, asserts DTR, and sends an address byte: `01h` selects the
controller and `81h` selects the memory card. Each later command byte receives
one response byte.

The current Phase 7 baseline includes:

- `JOY_DATA`, `JOY_STAT`, `JOY_MODE`, `JOY_CTRL`, and `JOY_BAUD` MMIO
- Baud-derived byte timing, RX FIFO state, delayed ACK/DSR pulses, and IRQ7
- Two independently connectable controller/card ports
- Digital controller ID `41h`, active-low buttons, and poll command `42h`
- DualShock analog ID `73h` with right-X, right-Y, left-X, and left-Y responses
- Raw 128 KiB memory-card images, formatted-card creation, and card flag state
- Memory-card Read (`52h`), Write (`57h`), and GetID (`53h`) commands
- 128-byte sectors, XOR checksums, invalid-sector responses, and dirty tracking
- Frontend persistence through `--memory-card` and `--memory-card2`

DualShock configuration and rumble commands, multitap support, nonstandard
controllers, and electrical signal-level timing are deferred until compatibility
testing requires them.
