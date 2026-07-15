# GTE

The Geometry Transformation Engine is COP2. CPU instructions move values with
`MTC2`/`MFC2` and `CTC2`/`CFC2`, then execute GTE commands through COP2 command
opcodes.

The current implementation provides the Phase 6 baseline:

- Data/control register special cases: sign extension, zero extension,
  `SXY` FIFO pushes, `IRGB`/`ORGB`, `LZCS`/`LZCR`, and `FLAG` summary bits
- COP2 command dispatch from the CPU interpreter
- `BC2F` always branches and `BC2T` never branches, matching the fixed false
  COP2 condition
- `RTPS` and `RTPT` perspective transforms with screen/depth FIFO updates
- Unsigned `H` projection input and the hardware UNR reciprocal approximation
- `NCLIP` screen-space triangle area
- `AVSZ3` and `AVSZ4` average depth/OTZ calculation
- `MVMVA` matrix/vector multiply-add across rotation, light, light color, the
  reserved `mx=3` matrix shape, and the `cv=2` far-color quirk
- Arithmetic commands: `SQR`, `OP`, `GPF`, and `GPL`
- Color/depth interpolation commands: `DPCS`, `DPCT`, `DCPL`, and `INTPL`
- Lighting/color commands: `NCS`, `NCT`, `NCCS`, `NCCT`, `NCDS`, `NCDT`,
  `CC`, and `CDP`
- Documented command latency, independent CPU overlap, and interlocks before
  GTE result reads, stores, and subsequent commands

Still pending:

- Saturation and overflow corner cases beyond the native regression set
- Per-register GTE load/store and internal pipeline timing hazards
- Hardware-test or reference-emulator comparison for the full command family
