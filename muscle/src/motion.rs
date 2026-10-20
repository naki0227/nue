//! Virtual camera for one segment: punch-ins, slow push-ins, eased emphasis
//! zooms, shake, and the picture half of edge transitions (zoom, whip, dip),
//! combined into one per-frame scale + crop so every effect is smooth.
//!
//! The camera is a view zoom `v(t)` (1 = the normal reframed picture) and a
//! horizontal/vertical offset in output pixels. `zoom_at` evaluates the same
//! curves in Rust as the FFmpeg expressions do, for tests and planning.

use serde::{Deserialize, Serialize};

/// How a cut boundary is animated. Applied to the end of the outgoing
/// segment and the start of the incoming one, so the timeline never shifts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Transition {
    /// Hard cut.
    #[default]
    Cut,
    /// Quick dip through white (flash) or black.
    DipWhite,
    DipBlack,
    /// Zoom through: accelerate into the cut, decelerate out of it, with blur.
    Zoom,
    /// Whip pan: fast horizontal slide with motion blur.
    Whip,
    /// RGB split + noise for a few frames.
    Glitch,
    /// Blur out and back in.
    Blur,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Move {
    /// Constant zoom for the whole segment (alternating jump-cut framing).
    Punch(f64),
    /// Linear push from 1 to `amount` over the segment.
    Push(f64),
    /// Eased zoom to `amount` between `start` and `end` (segment seconds).
    Zoom { amount: f64, start: f64, end: f64 },
    /// Handheld-style shake between `start` and `end`, amplitude in output px.
    Shake {
        amplitude: f64,
        start: f64,
        end: f64,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Camera {
    pub duration: f64,
    pub moves: Vec<Move>,
    /// Transition into this segment (from the previous one) and its length.
    pub enter: (Transition, f64),
    /// Transition out of this segment (to the next one) and its length.
    pub exit: (Transition, f64),
}

const ZOOM_EASE_IN: f64 = 0.12;
const ZOOM_EASE_OUT: f64 = 0.18;
const TRANSITION_ZOOM: f64 = 0.35;
const WHIP_ZOOM: f64 = 0.4;

impl Camera {
    #[cfg(test)]
    pub fn is_static(&self) -> bool {
        self.moves.is_empty() && !self.enter.0.moves_camera() && !self.exit.0.moves_camera()
    }

    #[cfg(test)]
    /// Progress 0..1 through the incoming transition at `t` (1 = done).
    fn enter_progress(&self, t: f64) -> f64 {
        ((t / self.enter.1.max(1e-3)).clamp(0.0, 1.0)).max(if self.enter.0.moves_camera() {
            0.0
        } else {
            1.0
        })
    }

    #[cfg(test)]
    /// Progress 0..1 through the outgoing transition at `t` (0 = not started).
    fn exit_progress(&self, t: f64) -> f64 {
        if !self.exit.0.moves_camera() {
            return 0.0;
        }
        ((t - (self.duration - self.exit.1)) / self.exit.1.max(1e-3)).clamp(0.0, 1.0)
    }

    #[cfg(test)]
    /// View zoom at `t` seconds into the segment.
    pub fn zoom_at(&self, t: f64) -> f64 {
        let mut v = 1.0;
        for m in &self.moves {
            v *= match *m {
                Move::Punch(z) => z,
                Move::Push(z) => 1.0 + (z - 1.0) * (t / self.duration.max(1e-3)).clamp(0.0, 1.0),
                Move::Zoom { amount, start, end } => 1.0 + (amount - 1.0) * envelope(t, start, end),
                Move::Shake { .. } => 1.04,
            };
        }
        let enter = 1.0 - self.enter_progress(t);
        let exit = self.exit_progress(t);
        v *= 1.0 + transition_zoom(self.enter.0) * ease_out_sq(enter);
        v *= 1.0 + transition_zoom(self.exit.0) * exit * exit;
        v
    }

    /// FFmpeg expression for the view zoom at time `t` (seconds).
    fn zoom_expr(&self) -> String {
        let t = "t";
        let mut factors = Vec::new();
        for m in &self.moves {
            factors.push(match *m {
                Move::Punch(z) => format!("{z:.4}"),
                Move::Push(z) => format!(
                    "(1+{:.4}*clip({t}/{:.4},0,1))",
                    z - 1.0,
                    self.duration.max(1e-3)
                ),
                Move::Zoom { amount, start, end } => {
                    format!("(1+{:.4}*{})", amount - 1.0, envelope_expr(t, start, end))
                }
                Move::Shake { .. } => "1.04".to_string(),
            });
        }
        if self.enter.0.moves_camera() {
            factors.push(format!(
                "(1+{:.4}*pow(1-clip({t}/{:.4},0,1),2))",
                transition_zoom(self.enter.0),
                self.enter.1.max(1e-3)
            ));
        }
        if self.exit.0.moves_camera() {
            factors.push(format!(
                "(1+{:.4}*pow(clip(({t}-{:.4})/{:.4},0,1),2))",
                transition_zoom(self.exit.0),
                self.duration - self.exit.1,
                self.exit.1.max(1e-3)
            ));
        }
        if factors.is_empty() {
            "1".to_string()
        } else {
            factors.join("*")
        }
    }

    /// Horizontal camera offset in output pixels at time `t`.
    fn x_offset_expr(&self, width: u32) -> String {
        let mut terms = Vec::new();
        if self.enter.0 == Transition::Whip {
            // Arrive from the left, decelerating.
            terms.push(format!(
                "-{:.1}*pow(1-clip(t/{:.4},0,1),2)",
                width as f64 * 0.45,
                self.enter.1.max(1e-3)
            ));
        }
        if self.exit.0 == Transition::Whip {
            // Leave to the right, accelerating.
            terms.push(format!(
                "{:.1}*pow(clip((t-{:.4})/{:.4},0,1),2)",
                width as f64 * 0.45,
                self.duration - self.exit.1,
                self.exit.1.max(1e-3)
            ));
        }
        for m in &self.moves {
            if let Move::Shake {
                amplitude,
                start,
                end,
            } = *m
            {
                // Two incommensurate sines read as handheld noise; decays over the window.
                terms.push(format!(
                    "{amplitude:.1}*(sin(2*PI*11*t)+0.4*sin(2*PI*23*t))*clip(({end:.3}-t)/{:.3},0,1)*between(t,{start:.3},{end:.3})",
                    (end - start).max(0.05)
                ));
            }
        }
        if terms.is_empty() {
            "0".to_string()
        } else {
            terms.join("+")
        }
    }

    fn y_offset_expr(&self) -> String {
        let terms: Vec<String> = self
            .moves
            .iter()
            .filter_map(|m| match *m {
                Move::Shake { amplitude, start, end } => Some(format!(
                    "{:.1}*(cos(2*PI*13*t)+0.4*sin(2*PI*29*t))*clip(({end:.3}-t)/{:.3},0,1)*between(t,{start:.3},{end:.3})",
                    amplitude * 0.8,
                    (end - start).max(0.05)
                )),
                _ => None,
            })
            .collect();
        if terms.is_empty() {
            "0".to_string()
        } else {
            terms.join("+")
        }
    }

    /// Scale + crop chain that turns the source (displayed `source` size) into
    /// `frame` following the camera. The source is rescaled every frame with
    /// lanczos, so zooms never enlarge an already-scaled picture, and there is
    /// none of zoompan's integer jitter. `center` is the subject's horizontal
    /// center (0..1, may depend on `t` for pans).
    pub fn filter(&self, frame: (u32, u32), source: (u32, u32), center: &str) -> String {
        let (w, h) = frame;
        let (sw, sh) = (source.0.max(1) as f64, source.1.max(1) as f64);
        let fill = (w as f64 / sw).max(h as f64 / sh);
        let (bw, bh) = (sw * fill, sh * fill);
        let z = self.zoom_expr();
        let x = self.x_offset_expr(w);
        let y = self.y_offset_expr();
        format!(
            "scale=w='2*trunc({bw:.2}*({z})/2+0.5)':h='2*trunc({bh:.2}*({z})/2+0.5)':eval=frame:flags=lanczos,\
             crop={w}:{h}:x='clip(iw*({center})-ow/2+{x},0,iw-ow)':y='clip((ih-oh)/2+{y},0,ih-oh)'"
        )
    }

    /// Picture effects layered on top of the camera at the edges (dips,
    /// motion blur, glitch). Returns comma-terminated filters or "".
    pub fn edge_filters(&self, width: u32) -> String {
        let mut f = String::new();
        let d = self.duration;
        let blur = |a: f64, b: f64, horizontal: bool| {
            let sigma_v = if horizontal { 1.0 } else { width as f64 / 60.0 };
            format!(
                "gblur=sigma={:.1}:sigmaV={sigma_v:.1}:enable='between(t,{a:.3},{b:.3})',",
                width as f64 / 40.0
            )
        };
        let (enter, enter_len) = self.enter;
        let (exit, exit_len) = self.exit;
        // Motion blur on the fastest part of camera transitions.
        match enter {
            Transition::Zoom | Transition::Blur => f.push_str(&blur(0.0, enter_len * 0.5, false)),
            Transition::Whip => f.push_str(&blur(0.0, enter_len * 0.6, true)),
            _ => {}
        }
        match exit {
            Transition::Zoom | Transition::Blur => f.push_str(&blur(d - exit_len * 0.5, d, false)),
            Transition::Whip => f.push_str(&blur(d - exit_len * 0.6, d, true)),
            _ => {}
        }
        for (transition, a, b) in [(enter, 0.0, enter_len), (exit, d - exit_len, d)] {
            if transition == Transition::Glitch {
                f.push_str(&format!(
                    "rgbashift=rh=-14:bh=14:enable='between(t,{a:.3},{b:.3})',\
                     noise=alls=30:allf=t:enable='between(t,{a:.3},{b:.3})',"
                ));
            }
        }
        // Dips are fades to a colour; the incoming segment fades back from it.
        match enter {
            Transition::DipWhite => {
                f.push_str(&format!("fade=t=in:st=0:d={enter_len:.3}:color=white,"))
            }
            Transition::DipBlack => {
                f.push_str(&format!("fade=t=in:st=0:d={enter_len:.3}:color=black,"))
            }
            _ => {}
        }
        match exit {
            Transition::DipWhite => f.push_str(&format!(
                "fade=t=out:st={:.3}:d={exit_len:.3}:color=white,",
                (d - exit_len).max(0.0)
            )),
            Transition::DipBlack => f.push_str(&format!(
                "fade=t=out:st={:.3}:d={exit_len:.3}:color=black,",
                (d - exit_len).max(0.0)
            )),
            _ => {}
        }
        f
    }
}

impl Transition {
    pub fn moves_camera(self) -> bool {
        matches!(self, Transition::Zoom | Transition::Whip)
    }
}

fn transition_zoom(t: Transition) -> f64 {
    match t {
        Transition::Zoom => TRANSITION_ZOOM,
        Transition::Whip => WHIP_ZOOM,
        _ => 0.0,
    }
}

#[cfg(test)]
fn ease_out_sq(x: f64) -> f64 {
    x * x
}

#[cfg(test)]
/// 0 -> 1 eased in over ZOOM_EASE_IN after `start`, held, back to 0 by `end`.
fn envelope(t: f64, start: f64, end: f64) -> f64 {
    let rise = ((t - start) / ZOOM_EASE_IN).clamp(0.0, 1.0);
    let fall = ((end - t) / ZOOM_EASE_OUT).clamp(0.0, 1.0);
    let x = rise.min(fall);
    // easeOutCubic for a snappy punch.
    1.0 - (1.0 - x).powi(3)
}

fn envelope_expr(t: &str, start: f64, end: f64) -> String {
    format!(
        "(1-pow(1-min(clip(({t}-{start:.4})/{ZOOM_EASE_IN},0,1),clip(({end:.4}-{t})/{ZOOM_EASE_OUT},0,1)),3))"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn camera(moves: Vec<Move>, enter: Transition, exit: Transition) -> Camera {
        Camera {
            duration: 4.0,
            moves,
            enter: (enter, 0.3),
            exit: (exit, 0.3),
        }
    }

    #[test]
    fn static_camera_stays_at_one() {
        let c = camera(vec![], Transition::Cut, Transition::DipWhite);
        assert!(c.is_static());
        assert_eq!(c.zoom_at(2.0), 1.0);
    }

    #[test]
    fn emphasis_zoom_eases_in_and_out() {
        let c = camera(
            vec![Move::Zoom {
                amount: 1.25,
                start: 1.0,
                end: 2.0,
            }],
            Transition::Cut,
            Transition::Cut,
        );
        assert_eq!(c.zoom_at(0.5), 1.0);
        assert!((c.zoom_at(1.5) - 1.25).abs() < 1e-9);
        let mid_rise = c.zoom_at(1.06);
        assert!(mid_rise > 1.0 && mid_rise < 1.25);
        assert_eq!(c.zoom_at(2.5), 1.0);
    }

    #[test]
    fn zoom_transition_accelerates_into_the_cut() {
        let c = camera(vec![Move::Push(1.06)], Transition::Zoom, Transition::Zoom);
        assert!(c.zoom_at(0.0) > 1.3, "arrives zoomed in");
        assert!(c.zoom_at(3.99) > 1.35, "leaves zoomed in");
        assert!((c.zoom_at(2.0) - 1.03).abs() < 1e-6, "push-in halfway");
    }

    #[test]
    fn expressions_reference_frame_time() {
        let c = camera(
            vec![Move::Shake {
                amplitude: 12.0,
                start: 1.0,
                end: 1.5,
            }],
            Transition::Whip,
            Transition::Cut,
        );
        let f = c.filter((1080, 1920), (1920, 1080), "0.5");
        assert!(f.contains("eval=frame"));
        assert!(f.contains("sin(2*PI*11*t)"));
        assert!(f.contains("crop=1080:1920"));
        // Fill size: 1920x1080 scaled to cover 1080x1920 -> 3413.33 x 1920.
        assert!(f.contains("3413.33"), "{f}");
        assert!(c.edge_filters(1080).contains("gblur"));
    }

    #[test]
    fn dips_fade_through_colour() {
        let c = camera(vec![], Transition::DipWhite, Transition::DipBlack);
        let f = c.edge_filters(1080);
        assert!(f.contains("fade=t=in:st=0:d=0.300:color=white"));
        assert!(f.contains("fade=t=out:st=3.700:d=0.300:color=black"));
    }
}

#[cfg(test)]
mod render_check {
    use super::*;

    /// Renders a 2 s clip through the camera filter (skipped without ffmpeg).
    #[test]
    fn camera_filter_renders() {
        if std::process::Command::new("ffmpeg")
            .arg("-version")
            .output()
            .is_err()
        {
            return;
        }
        let c = Camera {
            duration: 2.0,
            moves: vec![
                Move::Push(1.06),
                Move::Shake {
                    amplitude: 18.0,
                    start: 0.8,
                    end: 1.2,
                },
            ],
            enter: (Transition::Zoom, 0.3),
            exit: (Transition::Whip, 0.3),
        };
        let vf = format!(
            "{},{}null",
            c.filter((540, 960), (1280, 720), "0.6"),
            c.edge_filters(540)
        );
        let out = std::env::temp_dir().join(format!("nue-cam-{}.mp4", std::process::id()));
        let status = std::process::Command::new("ffmpeg")
            .args([
                "-y",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=s=1280x720:r=30:d=2",
                "-vf",
            ])
            .arg(&vf)
            .args(["-c:v", "libx264", "-preset", "ultrafast"])
            .arg(&out)
            .output()
            .unwrap();
        assert!(
            status.status.success(),
            "{}\n{vf}",
            String::from_utf8_lossy(&status.stderr)
        );
        let probe = std::process::Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-count_frames",
                "-show_entries",
                "stream=width,height,nb_read_frames",
                "-of",
                "csv=p=0",
            ])
            .arg(&out)
            .output()
            .unwrap();
        assert_eq!(String::from_utf8_lossy(&probe.stdout).trim(), "540,960,60");
        let _ = std::fs::remove_file(out);
    }
}
