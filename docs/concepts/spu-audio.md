# SPU Audio

The SPU owns 512 KiB of sound RAM and produces stereo signed-16 PCM at 44.1 kHz,
exactly one sample frame per 768 CPU cycles. Sound RAM is not CPU-mapped; CPU
software reaches it through the transfer FIFO or DMA channel 4.

The Phase 8 baseline implements:

- The voice, global control, status, transfer, internal-volume, and reverb MMIO
  ranges, including 16-bit and paired 32-bit accesses
- Manual FIFO transfers, DMA4 reads/writes, transfer modes and request status,
  and a shared incrementing sound-RAM address
- IRQ9 on matching transfer, voice-block, capture, and reverb RAM accesses
- 24 SPU-ADPCM voices with five predictor filters and history across blocks
- Pitch stepping, pitch modulation, loop flags, repeat addresses, `ENDX`, and
  key on/off behavior
- Attack, decay, sustain, and release envelopes plus fixed/swept stereo volume
- Shared hardware-style noise generation and per-voice noise selection
- Voice/CD dry mixing, CD and voice 1/3 capture buffers, and CD volume/bypass
- Reverb sends and the documented half-rate sound-RAM reflection/comb/APF path
- A bounded interleaved output queue exposed through `Ps1::drain_audio`
- Headless WAV capture through frontend `--audio-dump PATH`

The CD input accepts stereo PCM, but the CD-ROM device does not yet decode XA or
CD-DA into that input. Exact four-tap Gaussian interpolation, sub-sample register
application timing, inactive-voice RAM reads, transfer contention/latency,
unstable misconfigured reads, external audio, and reverb bus-order edge cases
remain compatibility work.

Reference: https://psx-spx.consoledev.net/soundprocessingunitspu/
