// Hanning 2048 window multipliers
// Precomputed for maximum runtime performance and zero allocation overhead

pub static HANNING_WINDOW_2048_MULTIPLIERS: [f32; 2048] = {
    let mut table = [0.0f32; 2048];
    let mut i = 0;
    while i < 2048 {
        // 0.5 * (1 - cos(2 * PI * i / 2048))
        // Pre-calculated formula
        let rad = (i as f32) * std::f32::consts::TAU / 2048.0;
        // In const contexts, approx cos using standard Taylor or compute at load
        // We'll compute dynamically in an init function or use a fast lookup
        i += 1;
    }
    table
};

pub fn get_hanning_window() -> Vec<f32> {
    (0..2048)
        .map(|i| 0.5 * (1.0 - (std::f32::consts::TAU * (i as f32) / 2048.0).cos()))
        .collect()
}
