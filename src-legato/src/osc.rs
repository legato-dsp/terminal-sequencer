use ratatui::widgets::canvas::{Canvas, Line};
use ratatui::widgets::{Widget, WidgetRef};

use crate::DISPLAY_SAMPLES;

pub struct Oscilloscope {
    osc_buffer: [f32; DISPLAY_SAMPLES],
}

impl Default for Oscilloscope {
    fn default() -> Self {
        Self {
            osc_buffer: [0.0; DISPLAY_SAMPLES],
        }
    }
}

impl Oscilloscope {
    pub fn update(&mut self, samples: &[f32]) {
        let trigger = find_trigger(samples, 0.0, 0.002, 3).unwrap_or(0); // The start point of the wave that is shown

        let src = &samples[trigger..trigger + DISPLAY_SAMPLES];

        self.osc_buffer.copy_from_slice(src); // TODO: Remove copy maybe with just a lifetime and constructing each call?
    }
}

impl WidgetRef for Oscilloscope {
    fn render_ref(&self, area: ratatui::prelude::Rect, buf: &mut ratatui::prelude::Buffer) {
        let max = self
            .osc_buffer
            .iter()
            .max_by(|x, y| x.partial_cmp(y).unwrap())
            .unwrap_or(&1.0)
            .clamp(0.1, 1.0);

        let min = self
            .osc_buffer
            .iter()
            .min_by(|x, y| x.partial_cmp(y).unwrap())
            .unwrap_or(&-1.0)
            .clamp(-1.0, -0.1);

        let block = ratatui::widgets::Block::bordered()
            .title(" Waveform ")
            .border_style(ratatui::style::Style::default().fg(ratatui::style::Color::LightCyan));

        let res = Canvas::default()
            .block(block)
            .x_bounds([0.0, DISPLAY_SAMPLES as f64])
            .y_bounds([min as f64, max as f64])
            .paint(move |ctx| {
                let display_data = &self.osc_buffer;
                for i in 0..display_data.len().saturating_sub(1) {
                    ctx.draw(&Line {
                        x1: i as f64,
                        y1: display_data[i] as f64,
                        x2: (i + 1) as f64,
                        y2: display_data[i + 1] as f64,
                        color: ratatui::style::Color::Cyan,
                    });
                }
            });

        res.render(area, buf);
    }
}

fn find_trigger(samples: &[f32], threshold: f32, dampening: f32, depth: usize) -> Option<usize> {
    let low = threshold - dampening;
    let high = threshold + dampening;

    let mut armed = false;

    // Only search first half so there's always DISPLAY_SAMPLES after the trigger
    for i in 0..samples.len().saturating_sub(depth + DISPLAY_SAMPLES) {
        if samples[i] < low {
            armed = true;
        }
        if armed && samples[i] >= high {
            // Confirm `depth` consecutive samples stay above threshold
            let confirmed =
                (1..=depth).all(|d| samples.get(i + d).map_or(false, |&s| s >= threshold));
            if confirmed {
                return Some(i);
            }
        }
    }
    None
}
