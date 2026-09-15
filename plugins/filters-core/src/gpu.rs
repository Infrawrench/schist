//! Shader companions for the CPU filter bodies. Parameters are prepared
//! in the effect logic, after the same slider normalization as the CPU.
//! See `crates/fx/README.md` for the ABI and how to add another effect.

pub use schist_fx::try_shader_rgba as apply;
use schist_fx::ShaderSpec;

macro_rules! shaders {
    ($($name:ident => $file:literal),* $(,)?) => {
        $(pub static $name: ShaderSpec = ShaderSpec {
            name: $file,
            source: include_str!(concat!("shaders/", $file, ".wgsl")),
        };)*
        /// Also used by offline validation, so every registered source is checked.
        pub static SHADERS: &[&ShaderSpec] = &[$(&$name),*];
    };
}

shaders! {
    CONVOLVE => "convolve",
    MORPHOLOGY => "morphology",
    BILATERAL => "bilateral",
    MEDIAN => "median",
    MOTION => "motion",
    RADIAL => "radial",
    OIL => "oil",
    FIND_EDGES => "find_edges",
    TRACE_CONTOUR => "trace_contour",
    FACET => "facet",
    ADD_NOISE => "add_noise",
    OFFSET => "offset",
    TWIRL => "twirl",
    RIPPLE => "ripple",
    WAVE => "wave",
    EMBOSS => "emboss",
    FRAGMENT => "fragment",
    CLOUDS => "clouds",
}

/// Median's per-invocation scratch array is bounded to keep register use
/// reasonable. Larger windows continue through the existing CPU body.
pub fn median(
    px: &mut [f32],
    w: usize,
    h: usize,
    r: i32,
    disc: bool,
    channels: usize,
    threshold: f32,
) -> bool {
    if !(1..=4).contains(&r) {
        return false;
    }
    let taps = (2 * r + 1).pow(2) as usize;
    apply(
        px,
        w,
        h,
        &MEDIAN,
        &[r as f32, disc as u8 as f32, channels as f32, threshold],
        Some(r as usize),
        taps * 8,
    )
}

pub fn bilateral(px: &mut [f32], w: usize, h: usize, r: i32, threshold: f32, disc: bool) -> bool {
    apply(
        px,
        w,
        h,
        &BILATERAL,
        &[r as f32, threshold, disc as u8 as f32],
        Some(r as usize),
        (2 * r + 1).pow(2) as usize,
    )
}
