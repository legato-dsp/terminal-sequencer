#![feature(iter_collect_into)]

use std::collections::VecDeque;
use std::fs;
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
use ratatui::DefaultTerminal;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::{Widget, WidgetRef};

use crate::osc::Oscilloscope;

mod osc;

pub const RING_SIZE: usize = 4096;
pub const DISPLAY_SAMPLES: usize = 512;

struct App {
    // SPSC consumer pulling from the final state of the graph
    consumer: rtrb::Consumer<f32>,
    // A second ring buffer to pull samples into so we can visualize
    visualization_ring: VecDeque<f32>,
    oscilloscope: Oscilloscope,
}

impl App {
    pub fn new(consumer: rtrb::Consumer<f32>) -> Self {
        Self {
            consumer,
            visualization_ring: VecDeque::from(vec![0.0; RING_SIZE]),
            oscilloscope: Oscilloscope::default(),
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

        self.oscilloscope.update(samples);

        self.oscilloscope.render_ref(area, buf);
    }
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
    let graph = fs::read_to_string("../.legato").expect("Could not fine legato file!");

    let config = Config {
        sample_rate: 44_100,
        block_size: 4096,
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
