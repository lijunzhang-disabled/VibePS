# SPU Audio

The SPU has 512 KB of sound RAM and 24 ADPCM voices. Each voice has volume,
pitch, sample start/repeat addresses, ADSR envelope state, and optional noise,
pitch modulation, and reverb routing.

The first implementation step is correct SPU RAM transfer behavior through
manual writes and DMA4. Actual voice decoding, ADSR, interpolation, and reverb
come after BIOS/CD/GPU bring-up.

