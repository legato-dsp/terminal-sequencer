#![feature(iter_collect_into)]

use std::collections::VecDeque;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode};
use legato::interface::AudioInterface;
use legato::midi::{MidiPortKind, start_midi_thread};
use legato::{
    builder::{LegatoBuilder, Unconfigured},
    config::Config,
    out::start_application_audio_thread_external_output,
    ports::PortBuilder,
};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::Widget;
use ratatui::widgets::canvas::{Canvas, Line};
use ratatui::{DefaultTerminal, Frame, Terminal};

const RING_SIZE: usize = 4096;
const DISPLAY_SAMPLES: usize = 512;

struct App {
    // SPSC consumer pulling from the final state of the graph
    consumer: rtrb::Consumer<f32>,
    // A second ring buffer to pull samples into so we can visualize
    visualization_ring: VecDeque<f32>,
    // Preallocated display buffer
    osc_buffer: [f32; DISPLAY_SAMPLES],
}

impl App {
    pub fn new(consumer: rtrb::Consumer<f32>) -> Self {
        Self {
            consumer,
            visualization_ring: VecDeque::from(vec![0.0; RING_SIZE]),
            osc_buffer: [0.0; DISPLAY_SAMPLES],
        }
    }
    pub fn run(&mut self, terminal: &mut DefaultTerminal) -> std::io::Result<()> {
        terminal.draw(|frame| {
            frame.render_widget(&mut *self, frame.area());
        })?;
        Ok(())
    }
}

impl Widget for &mut App {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Drain all the available samples
        let new_samples: usize = self.consumer.slots();

        for _ in 0..new_samples {
            if let Ok(sample) = self.consumer.pop() {
                self.visualization_ring.pop_front();
                self.visualization_ring.push_back(sample);
            }
        }

        let samples = self.visualization_ring.make_contiguous();

        let trigger = find_trigger(samples, 0.0, 0.002, 3).unwrap_or(0); // The start point of the wave that is shown

        let src = &samples[trigger..trigger + DISPLAY_SAMPLES];

        self.osc_buffer.copy_from_slice(src);

        let max = samples
            .iter()
            .max_by(|x, y| x.partial_cmp(y).unwrap())
            .unwrap_or(&1.0)
            .clamp(0.1, 1.0);

        let min = samples
            .iter()
            .min_by(|x, y| x.partial_cmp(y).unwrap())
            .unwrap_or(&-1.0)
            .clamp(-1.0, -0.1);

        let block = ratatui::widgets::Block::bordered()
            .title(" Waveform ")
            .border_style(ratatui::style::Style::default().fg(ratatui::style::Color::LightMagenta));

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
                        color: ratatui::style::Color::LightMagenta,
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

fn start_ui_thread(mut app: App) -> Result<(), Box<dyn std::error::Error>> {
    ratatui::run(|terminal| {
        loop {
            if event::poll(Duration::from_millis(1))? {
                if let Event::Key(key) = event::read()? {
                    if key.code == KeyCode::Char('q') {
                        return Ok(());
                    }
                }
            }

            app.run(terminal)?;
        }
    })
}

fn start_legato_runtime(producer: rtrb::Producer<f32>) -> () {
    let graph = String::from(
        r#"
        patch voice(
            attack = 20.0,
            decay = 80.0,
            sustain = 0.1,
            release = 60.0
        ) {
            in freq gate

            audio {
                sine: mod,
                sine: carrier,
                adsr { attack: $attack, decay: $decay, sustain: $sustain, release: $release, chans: 1 },
                mult: freq_mult,
                mult: fm_gain { val: 1.0 },
                add: fm_add
            }

            control {
                signal: ratio { name: "ratio", min: 1.0, max: 100.0, default: 1.5 }
            }

            freq >> freq_mult[0]

            ratio >> freq_mult[1]

            freq_mult >> mod.freq

            mod >> fm_gain[0]


            freq >> fm_add[0]
            fm_gain >> fm_add[1]

            fm_add >> carrier.freq

            gate >> adsr.gate

            carrier >> adsr[1]

            { adsr }
        }

        patches {
            voice * 5 { }
        }

        audio {
            track_mixer: osc_mixer { tracks: 5, chans_per_track: 1, gain: [0.1, 0.1, 0.1, 0.1, 0.1] },
            mono_fan_out { chans: 2 },

            delay_write: dw1 { delay_name: "d_one", delay_length: 2000.0, chans: 2 },
            delay_read: dr1 { delay_name: "d_one", chans: 2, delay_length: [ 938, 731 ] },
            delay_read: dr2 { delay_name: "d_one", chans: 2, delay_length: [ 459, 643 ] },

            track_mixer: master { tracks: 3, chans_per_track: 2, gain: [0.4, 0.5, 0.5] },
            
            track_mixer: feedback { tracks: 2, chans_per_track: 2, gain: [0.2, 0.2] }
        }

        midi { 
            poly_voice { chan: 0, voices: 5 }
        }

        poly_voice[0:13:3] >> voice(*).gate
        poly_voice[1:13:3] >> voice(*).freq
        voice(*) >> osc_mixer[0..5]

        osc_mixer >> mono_fan_out

        mono_fan_out >> master[0..2]
        mono_fan_out >> dw1[0..2]

        dr1[0..2] >> master[2..4]
        dr2[0..2] >> master[4..6]

        // feedback    
        dr1 >> feedback[0..2]
        dr2 >> feedback[2..4]

        feedback >> dw1

        { master }
    "#,
    );

    let config = Config {
        sample_rate: 48_000,
        block_size: 256,
        channels: 2,
        rt_capacity: 0,
    };

    let (midi_rt_fe, _writer_fe) = start_midi_thread(
        256,
        "my_port",
        MidiPortKind::Index(0),
        MidiPortKind::Index(0),
        "my_port",
    )
    .unwrap();

    let ports = PortBuilder::default().audio_out(2).build();

    let (app, _) = LegatoBuilder::<Unconfigured>::new(config, ports)
        .set_midi_runtime(midi_rt_fe)
        .build_dsl(&graph);

    let interface = AudioInterface::default_with_config(&config);

    start_application_audio_thread_external_output(interface, producer, app)
        .expect("Audio thread panic!")
}

fn main() {
    let (prod, consumer) = rtrb::RingBuffer::new(48_000);

    std::thread::spawn(|| {
        let _ = start_legato_runtime(prod);
        std::thread::park();
    });
    println!("Starting ui");

    let app = App::new(consumer);

    let _ = start_ui_thread(app);
}

// std::thread::spawn(move || {
//     std::thread::sleep(Duration::from_secs(5));
//     frontend.set_param("pitch", 880.0).unwrap();
// });
