use ps1_core::Ps1;

pub fn display_summary(ps1: &Ps1) -> String {
    let (width, height) = ps1.bus.gpu.display_size();
    format!("video={width}x{height}:BGR555")
}
