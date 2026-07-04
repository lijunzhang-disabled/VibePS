pub const AUDIO_RATE: u32 = 44_100;
pub const AUDIO_CHANNELS: u32 = 2;

pub fn audio_summary() -> String {
    format!("audio={AUDIO_RATE}hz:{AUDIO_CHANNELS}ch:s16le")
}
