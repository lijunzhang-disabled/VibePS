use ps1_core::{audio::SPU_OUTPUT_HZ, Ps1};
use std::fs::File;
use std::io::{Seek, SeekFrom, Write};
use std::path::Path;

pub const AUDIO_RATE: u32 = SPU_OUTPUT_HZ;
pub const AUDIO_CHANNELS: u16 = 2;

pub fn audio_summary(ps1: &Ps1) -> String {
    format!(
        "audio={AUDIO_RATE}hz:{AUDIO_CHANNELS}ch:s16le:queued={}",
        ps1.bus.spu.queued_samples() / AUDIO_CHANNELS as usize
    )
}

pub struct WavWriter {
    file: File,
    samples_written: u32,
}

impl WavWriter {
    pub fn create(path: &Path) -> Result<Self, String> {
        let mut file = File::create(path)
            .map_err(|err| format!("failed to create audio dump {}: {err}", path.display()))?;
        file.write_all(&wav_header(0))
            .map_err(|err| format!("failed to write audio dump {}: {err}", path.display()))?;
        Ok(Self {
            file,
            samples_written: 0,
        })
    }

    pub fn append(&mut self, samples: &[i16]) -> Result<(), String> {
        let mut bytes = Vec::with_capacity(samples.len() * 2);
        for sample in samples {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
        self.file
            .write_all(&bytes)
            .map_err(|err| format!("failed to append audio dump: {err}"))?;
        self.samples_written = self.samples_written.saturating_add(samples.len() as u32);
        Ok(())
    }

    pub fn finish(mut self) -> Result<(), String> {
        let data_bytes = self.samples_written.saturating_mul(2);
        self.file
            .seek(SeekFrom::Start(4))
            .and_then(|_| {
                self.file
                    .write_all(&36u32.saturating_add(data_bytes).to_le_bytes())
            })
            .and_then(|_| self.file.seek(SeekFrom::Start(40)))
            .and_then(|_| self.file.write_all(&data_bytes.to_le_bytes()))
            .and_then(|_| self.file.flush())
            .map_err(|err| format!("failed to finalize audio dump: {err}"))
    }
}

fn wav_header(data_bytes: u32) -> [u8; 44] {
    let mut header = [0u8; 44];
    let block_align = AUDIO_CHANNELS * 2;
    let byte_rate = AUDIO_RATE * block_align as u32;
    header[0..4].copy_from_slice(b"RIFF");
    header[4..8].copy_from_slice(&36u32.saturating_add(data_bytes).to_le_bytes());
    header[8..12].copy_from_slice(b"WAVE");
    header[12..16].copy_from_slice(b"fmt ");
    header[16..20].copy_from_slice(&16u32.to_le_bytes());
    header[20..22].copy_from_slice(&1u16.to_le_bytes());
    header[22..24].copy_from_slice(&AUDIO_CHANNELS.to_le_bytes());
    header[24..28].copy_from_slice(&AUDIO_RATE.to_le_bytes());
    header[28..32].copy_from_slice(&byte_rate.to_le_bytes());
    header[32..34].copy_from_slice(&block_align.to_le_bytes());
    header[34..36].copy_from_slice(&16u16.to_le_bytes());
    header[36..40].copy_from_slice(b"data");
    header[40..44].copy_from_slice(&data_bytes.to_le_bytes());
    header
}

#[cfg(test)]
mod tests {
    use super::wav_header;

    #[test]
    fn wav_header_describes_44100_hz_stereo_s16() {
        let header = wav_header(400);

        assert_eq!(&header[0..4], b"RIFF");
        assert_eq!(&header[8..12], b"WAVE");
        assert_eq!(u16::from_le_bytes([header[22], header[23]]), 2);
        assert_eq!(
            u32::from_le_bytes(header[24..28].try_into().unwrap()),
            44_100
        );
        assert_eq!(u32::from_le_bytes(header[40..44].try_into().unwrap()), 400);
    }
}
