# Online CPU Test Survey

## Source Checked

- PCSX-Redux test programs: https://github.com/grumpycoders/pcsx-redux/tree/main/src/mips/tests
- PCSX-Redux CPU tests: https://github.com/grumpycoders/pcsx-redux/tree/main/src/mips/tests/cpu
- PCSX-Redux runner references `cpu.ps-exe`, `cop0.ps-exe`, and OpenBIOS paths, but the shallow clone did not contain prebuilt `*.ps-exe` or `openbios.bin` artifacts.

## Local Build Attempt

```sh
make -C /private/tmp/pcsx-redux/src/mips/tests/cpu
```

Result: blocked because `mipsel-none-elf-gcc` / `mipsel-none-elf-g++` are not installed in this environment.

## Imported Into Core Tests

Two low-level PCSX-Redux CPU cases were ported into native Rust unit tests:

- A normal write to the same register cancels a pending delayed load.
- A `jal` link write to `$ra` cancels a pending delayed load to `$ra`.

These exposed and fixed a real issue in the interpreter: delayed loads were always committed after the following instruction, even when that instruction wrote the same target register.
