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
- `MVMVA` matrix/vector multiply-add across rotation, light, light color, the
  reserved `mx=3` matrix shape, and the `cv=2` far-color quirk
- Arithmetic commands: `SQR`, `OP`, `GPF`, and `GPL`
- Color/depth interpolation commands: `DPCS`, `DPCT`, `DCPL`, and `INTPL`
- Lighting/color commands: `NCS`, `NCT`, `NCCS`, `NCCT`, `NCDS`, `NCDT`,
  `CC`, and `CDP`

Still pending:

- More exact divider/table behavior for projection
- Saturation and overflow corner cases beyond the native regression set
- GTE command latency, CPU stalls, and register load timing
- Hardware-test or reference-emulator comparison for the full command family
