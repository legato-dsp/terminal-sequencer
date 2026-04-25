use ratatui::style::{Color, Style};
use ratatui::widgets::Block;
use ratatui::widgets::canvas::{Canvas, Line};
use ratatui::widgets::{Widget, WidgetRef};
use realfft::{RealFftPlanner, num_complex::Complex32};
use std::f32::consts::TAU;

const FFT_SIZE: usize = 2048;
const HOP_SIZE: usize = FFT_SIZE / 4; // TODO, try various resolutions

/// A reasonable window default for stft
///
/// https://en.wikipedia.org/wiki/Hann_function
fn hann(n: usize) -> Box<[f32]> {
    (0..n)
        .map(|i| 0.5 * (1.0 - ((TAU * i as f32) / n as f32).cos()))
        .collect::<Vec<_>>()
        .into()
}

pub struct Spectroscope {
    planner: RealFftPlanner<f32>,
    visualization_buffer: [f32; HOP_SIZE / 2 + 1], // Take first half up to nyquist
    // Apply the window before fft
    windowed_samples: [f32; HOP_SIZE],
    // The spectrum we right to
    spectrum: Box<[Complex32]>,
    window: Box<[f32]>,
}

impl Spectroscope {
    pub fn new() -> Self {
        Self {
            planner: RealFftPlanner::new(),
            visualization_buffer: [0.0; HOP_SIZE / 2 + 1],
            windowed_samples: [0.0; HOP_SIZE],
            spectrum: vec![Complex32::default(); HOP_SIZE / 2 + 1].into(),
            window: hann(HOP_SIZE),
        }
    }

    pub fn update(&mut self, samples: &[f32]) {
        debug_assert_eq!(samples.len() % HOP_SIZE, 0);

        for hop in samples.chunks_exact(HOP_SIZE) {
            self.compute_stft(hop);
        }

        for (normalized, out) in self
            .spectrum
            .iter()
            .map(|c| c.norm() / (HOP_SIZE / 2) as f32)
            .zip(self.visualization_buffer.iter_mut())
        {
            *out = normalized;
        }
    }

    pub fn compute_stft(&mut self, hop: &[f32]) {
        // Compute current window
        for i in 0..HOP_SIZE {
            self.windowed_samples[i] = hop[i] * self.window[i];
        }

        let real_to_complex = self.planner.plan_fft_forward(HOP_SIZE);

        // Am I wasting work here?
        real_to_complex
            .process(&mut self.windowed_samples, &mut self.spectrum)
            .unwrap();
    }
}

fn catmull_rom(p0: f32, p1: f32, p2: f32, p3: f32, t: f32) -> f32 {
    0.5 * ((2.0 * p1)
        + (-p0 + p2) * t
        + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t * t
        + (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * t * t * t)
}

fn catmull_rom_curve(points: &[f32], steps: usize) -> Vec<f32> {
    let n = points.len();
    if n < 2 {
        return points.to_vec();
    }

    let mut out = Vec::with_capacity((n - 1) * steps + 1);
    // Maybe upstream this to Legato, legato has nice utils for cubic + linear?
    // TODO: SIMD?
    for i in 0..n.saturating_sub(1) {
        let p0 = points[i.saturating_sub(1)];
        let p1 = points[i];
        let p2 = points[(i + 1).min(n - 1)];
        let p3 = points[(i + 2).min(n - 1)];
        for step in 0..steps {
            let t = step as f32 / steps as f32;
            out.push(catmull_rom(p0, p1, p2, p3, t).clamp(0.0, 1.0));
        }
    }
    out.push(*points.last().unwrap());
    out
}

impl WidgetRef for Spectroscope {
    fn render_ref(&self, area: ratatui::prelude::Rect, buf: &mut ratatui::prelude::Buffer) {
        let num_bins = self.visualization_buffer.len();
        let num_cols = area.width as usize;

        // One control point per character column, log-spaced over the bin array
        let control_points: Vec<f32> = (0..num_cols)
            .map(|col| {
                let t = col as f32 / num_cols as f32;
                let bin = ((num_bins as f32).powf(t)) as usize;
                let bin = bin.min(num_bins - 1);
                let magnitude = self.visualization_buffer[bin];
                const DB_FLOOR: f32 = -80.0;
                let db = if magnitude > 0.0 {
                    20.0 * magnitude.log10()
                } else {
                    DB_FLOOR
                };
                ((db - DB_FLOOR) / -DB_FLOOR).clamp(0.0, 1.0)
            })
            .collect();

        // Upsample 2x
        let curve = catmull_rom_curve(&control_points, 2);
        let curve_len = curve.len();

        let block = Block::bordered()
            .title(" Spectrum ")
            .border_style(Style::default().fg(Color::LightCyan));

        let canvas = Canvas::default()
            .block(block)
            .x_bounds([0.0, curve_len as f64])
            .y_bounds([0.0, 1.0])
            .paint(move |ctx| {
                for i in 0..curve_len.saturating_sub(1) {
                    ctx.draw(&Line {
                        x1: i as f64,
                        y1: 0.0,
                        x2: i as f64,
                        y2: curve[i] as f64,
                        color: Color::Cyan,
                    });
                    ctx.draw(&Line {
                        x1: i as f64,
                        y1: curve[i] as f64,
                        x2: (i + 1) as f64,
                        y2: curve[i + 1] as f64,
                        color: Color::LightCyan,
                    });
                }
            });

        canvas.render(area, buf);
    }
}
