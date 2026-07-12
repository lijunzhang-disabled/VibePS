# GTE

The Geometry Transformation Engine is COP2. CPU instructions move values with
`MTC2`/`MFC2` and `CTC2`/`CFC2`, then execute GTE commands through COP2 command
opcodes.

The current implementation provides the Phase 6 baseline:

- Data/control register special cases: sign extension, zero extension,
  `SXY` FIFO pushes, `IRGB`/`ORGB`, `LZCS`/`LZCR`, and `FLAG` summary bits
- COP2 command dispatch from the CPU interpreter
- `RTPS` and `RTPT` perspective transforms with screen/depth FIFO updates
- `NCLIP` screen-space triangle area
- `AVSZ3` and `AVSZ4` average depth/OTZ calculation
- Basic `MVMVA` matrix/vector multiply-add across rotation, light, and light
  color matrices

Still pending:

- Full lighting and color command family: `NCS`, `NCT`, `NCCS`, `NCCT`,
  `NCDS`, `NCDT`, `CC`, `CDP`, `DPCS`, `DPCT`, `DCPL`, and `INTPL`
- Remaining arithmetic commands: `SQR`, `OP`, `GPF`, and `GPL`
- Undocumented `MVMVA` edge cases such as `mx=3` and `cv=2`
- More exact divider, saturation, and pipeline timing behavior
