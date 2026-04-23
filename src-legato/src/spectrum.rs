use std::{collections::VecDeque, f32::consts::TAU};

use ratatui::widgets::{Widget, WidgetRef};
use realfft::{RealFftPlanner, num_complex::Complex32};

use crate::{DISPLAY_SAMPLES, RING_SIZE};

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
    ring: VecDeque<f32>,
    window: Box<[f32]>,
}

impl Spectroscope {
    pub fn new() -> Self {
        Self {
            planner: RealFftPlanner::new(),
            visualization_buffer: [0.0; HOP_SIZE / 2 + 1],
            windowed_samples: [0.0; HOP_SIZE],
            spectrum: vec![Complex32::default(); HOP_SIZE / 2 + 1].into(),
            ring: VecDeque::with_capacity(FFT_SIZE * 4),
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
            .map(|c| c.norm())
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

impl WidgetRef for Spectroscope {
    fn render_ref(&self, area: ratatui::prelude::Rect, buf: &mut ratatui::prelude::Buffer) {
        let block = ratatui::widgets::Block::bordered()
            .title(" Spectrum ")
            .border_style(ratatui::style::Style::default().fg(ratatui::style::Color::LightCyan));

        // Reserve the inner area for the bars, matching Oscilloscope's canvas inset
        let inner = block.inner(area);
        block.render(area, buf);

        let num_bins = self.visualization_buffer.len();
        let width = inner.width as usize;
        let height = inner.height as usize;

        for col in 0..width {
            let t = col as f32 / width as f32;
            let bin = ((num_bins as f32).powf(t)) as usize;
            let bin = bin.min(num_bins - 1);

            let magnitude = self.visualization_buffer[bin];

            const DB_FLOOR: f32 = -80.0;
            const DB_SCALE: f32 = 1.0 / -DB_FLOOR;
            let db = if magnitude > 0.0 {
                20.0 * magnitude.log10()
            } else {
                DB_FLOOR
            };
            let normalized = ((db - DB_FLOOR) * DB_SCALE).clamp(0.0, 1.0);
            let filled_rows = (normalized * height as f32).round() as usize;

            for row in 0..height {
                let is_filled = row >= height - filled_rows;
                if is_filled {
                    let x = inner.left() + col as u16;
                    let y = inner.top() + row as u16;
                    if let Some(cell) = buf.cell_mut((x, y)) {
                        cell.set_char('█').set_fg(ratatui::style::Color::Cyan);
                    }
                }
            }
        }
    }
}
