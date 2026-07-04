//! PS1 emulator frontend harness.
//!
//! This mirrors the sibling NDS frontend layout: `main` is only the binary
//! entrypoint, while harness/video/audio code lives in separate modules.

mod audio;
mod harness;
mod video;

fn main() {
    if let Err(message) = harness::run() {
        eprintln!("{message}");
        std::process::exit(1);
    }
}
