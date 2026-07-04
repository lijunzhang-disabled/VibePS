# COP0 Interrupt Mask

## Scope

- COP0 interrupt delivery now checks the full `SR.IM & Cause.IP` mask/pending field.
- External PS1 IRQ aggregation still updates `Cause.IP2`.
- Cause software interrupt bits 8-9 remain writable and can now trigger an interrupt when their matching status mask bit is enabled.

## Verification

Covered by a CPU unit test that writes `SR.IE|SR.IM0`, raises `Cause.Sw0`, and verifies the interrupt vector, EPC, and pending bit.
