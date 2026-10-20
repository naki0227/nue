//! Color grading and finishing ("the look"): a 3D LUT generated from a few
//! grading parameters, plus film grain, vignette and letterbox filters.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

/// Grading parameters. Defaults are neutral (identity LUT).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Grade {
    /// Overall gain (exposure), 1.0 = unchanged.
    pub exposure: f64,
    /// Contrast around mid grey, 1.0 = unchanged.
    pub contrast: f64,
    pub saturation: f64,
    /// Warm (+) / cool (-) white balance shift, about -1..1.
    pub temperature: f64,
    /// Green (+) / magenta (-) tint, about -1..1.
    pub tint: f64,
    /// Raised blacks for a faded, filmic floor (0..0.15).
    pub fade: f64,
    /// Highlight roll-off: brightest output value (0.85..1).
    pub white: f64,
    /// Split toning: RGB tint added to shadows and to highlights.
    pub shadow_tint: [f64; 3],
    pub highlight_tint: [f64; 3],
    /// Strength of the split toning, 0..1.
    pub split: f64,
}

impl Default for Grade {
    fn default() -> Self {
        Grade {
            exposure: 1.0,
            contrast: 1.0,
            saturation: 1.0,
            temperature: 0.0,
            tint: 0.0,
            fade: 0.0,
            white: 1.0,
            shadow_tint: [0.0; 3],
            highlight_tint: [0.0; 3],
            split: 0.0,
        }
    }
}

impl Grade {
    pub fn is_identity(&self) -> bool {
        *self == Grade::default()
    }

    /// Maps one normalized RGB value through the grade.
    pub fn apply(&self, rgb: [f64; 3]) -> [f64; 3] {
        let mut c = rgb.map(|v| v * self.exposure);
        c[0] *= 1.0 + 0.08 * self.temperature;
        c[2] *= 1.0 - 0.08 * self.temperature;
        c[1] *= 1.0 + 0.05 * self.tint;
        // Smooth S-curve: linear contrast blended with a sigmoid to keep ends soft.
        c = c.map(|v| {
            let linear = 0.5 + (v - 0.5) * self.contrast;
            let k = 4.0 * self.contrast;
            let sigmoid = 1.0 / (1.0 + (-k * (v - 0.5)).exp());
            let s0 = 1.0 / (1.0 + (k * 0.5).exp());
            let s1 = 1.0 / (1.0 + (-k * 0.5).exp());
            let sig = (sigmoid - s0) / (s1 - s0);
            if (self.contrast - 1.0).abs() < 1e-9 {
                v
            } else {
                0.5 * linear + 0.5 * sig
            }
        });
        let luma = 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
        c = c.map(|v| luma + (v - luma) * self.saturation);
        let l = luma.clamp(0.0, 1.0);
        let (ws, wh) = ((1.0 - l).powi(2), l.powi(2));
        for (i, v) in c.iter_mut().enumerate() {
            *v += self.split * 0.25 * (ws * self.shadow_tint[i] + wh * self.highlight_tint[i]);
        }
        c.map(|v| (self.fade + v.clamp(0.0, 1.0) * (self.white - self.fade)).clamp(0.0, 1.0))
    }

    /// Writes an Adobe/Resolve-compatible `.cube` 3D LUT (`size`³ entries).
    pub fn write_cube(&self, path: &Path, size: usize) -> Result<()> {
        let mut out = String::with_capacity(size * size * size * 28);
        writeln!(out, "TITLE \"nue\"")?;
        writeln!(out, "LUT_3D_SIZE {size}")?;
        let step = (size - 1) as f64;
        // .cube order: red changes fastest, then green, then blue.
        for b in 0..size {
            for g in 0..size {
                for r in 0..size {
                    let [ro, go, bo] =
                        self.apply([r as f64 / step, g as f64 / step, b as f64 / step]);
                    writeln!(out, "{ro:.6} {go:.6} {bo:.6}")?;
                }
            }
        }
        fs::write(path, out)?;
        Ok(())
    }
}

/// Finishing applied to every frame after reframing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Look {
    pub grade: Grade,
    /// Film grain strength (ffmpeg `noise` amount, 0 = none, 4-10 subtle).
    pub grain: u32,
    /// Vignette strength 0..1 (0 = none).
    pub vignette: f64,
    /// Letterbox aspect ratio, e.g. 2.39 (0 = none).
    pub letterbox: f64,
    /// Soft glow / halation on highlights, 0..1.
    pub glow: f64,
}

impl Look {
    /// Bloom graph from `[input]` to `[output]` (blurred copy screened back),
    /// or `None` without glow.
    pub fn glow_graph(&self, input: &str, output: &str, width: u32) -> Option<String> {
        (self.glow > 0.0).then(|| {
            format!(
                "[{input}]split[{input}g0][{input}g1];[{input}g1]gblur=sigma={:.1}[{input}g2];\
                 [{input}g0][{input}g2]blend=all_mode=screen:all_opacity={:.2}[{output}]",
                width as f64 / 90.0,
                (self.glow * 0.35).clamp(0.0, 0.5)
            )
        })
    }

    /// Linear FFmpeg filters (comma-separated, ending with a comma when not empty).
    pub fn filters(&self, lut: Option<&Path>, width: u32, height: u32) -> String {
        let mut f = String::new();
        if let Some(lut) = lut {
            let path = lut
                .to_string_lossy()
                .replace('\\', "/")
                .replace('\'', "'\\''");
            write!(f, "lut3d=file='{path}':interp=tetrahedral,").ok();
        }
        if self.vignette > 0.0 {
            // ffmpeg's angle: PI/5 is the default; larger is stronger.
            write!(
                f,
                "vignette=angle={:.3}:mode=forward,",
                0.2 + self.vignette * 0.5
            )
            .ok();
        }
        if self.grain > 0 {
            write!(f, "noise=c0s={}:c0f=t+u,", self.grain.min(40)).ok();
        }
        // Cinema bars only make sense on landscape frames.
        if self.letterbox > 0.0 && width > height {
            let picture = (width as f64 / self.letterbox).min(height as f64);
            let bar = (((height as f64 - picture) / 2.0) / 2.0).floor() as u32 * 2;
            if bar > 0 {
                write!(
                    f,
                    "drawbox=x=0:y=0:w=iw:h={bar}:color=black:t=fill,\
                     drawbox=x=0:y=ih-{bar}:w=iw:h={bar}:color=black:t=fill,"
                )
                .ok();
            }
        }
        f
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neutral_grade_is_identity() {
        let grade = Grade::default();
        assert!(grade.is_identity());
        let out = grade.apply([0.2, 0.5, 0.9]);
        for (a, b) in out.iter().zip([0.2, 0.5, 0.9]) {
            assert!((a - b).abs() < 1e-9);
        }
    }

    #[test]
    fn fade_lifts_blacks_and_warmth_shifts_balance() {
        let grade = Grade {
            fade: 0.08,
            temperature: 1.0,
            ..Grade::default()
        };
        assert!((grade.apply([0.0; 3])[0] - 0.08).abs() < 1e-9);
        let grey = grade.apply([0.5; 3]);
        assert!(
            grey[0] > grey[2],
            "warm grade should push red over blue: {grey:?}"
        );
    }

    #[test]
    fn split_tone_tints_shadows_and_highlights() {
        let grade = Grade {
            shadow_tint: [-0.2, 0.1, 0.3],
            highlight_tint: [0.3, 0.1, -0.2],
            split: 1.0,
            ..Grade::default()
        };
        let dark = grade.apply([0.15; 3]);
        let bright = grade.apply([0.85; 3]);
        assert!(dark[2] > dark[0], "shadows lean teal: {dark:?}");
        assert!(bright[0] > bright[2], "highlights lean orange: {bright:?}");
    }

    #[test]
    fn writes_cube_files() {
        let path = std::env::temp_dir().join(format!("nue-test-{}.cube", std::process::id()));
        Grade::default().write_cube(&path, 3).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("LUT_3D_SIZE 3"));
        assert_eq!(
            text.lines()
                .filter(|l| l.starts_with(|c: char| c.is_ascii_digit()))
                .count(),
            27
        );
        let _ = fs::remove_file(path);
    }

    #[test]
    fn letterbox_bars_are_even() {
        let look = Look {
            letterbox: 2.39,
            ..Look::default()
        };
        let f = look.filters(None, 1920, 1080);
        // 1920/2.39 = 803 px picture -> 138 px bars (even).
        assert!(f.contains("h=138"), "{f}");
    }
}
