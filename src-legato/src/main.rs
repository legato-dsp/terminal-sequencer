#![feature(iter_collect_into)]

use std::collections::VecDeque;
use std::fs;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode};
use legato::LegatoFrontend;
use legato::interface::AudioInterface;
use legato::midi::{MidiPortKind, start_midi_thread};
use legato::msg::StepPayload;
use legato::{
    builder::{LegatoBuilder, Unconfigured},
    config::Config,
    out::start_application_audio_thread_external_output,
    ports::PortBuilder,
};
use ratatui::DefaultTerminal;
use ratatui::buffer::Buffer;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget, WidgetRef};

use crate::osc::Oscilloscope;
use crate::spectrum::Spectroscope;
use crate::tracker::{Column, Tracker, freq_to_note_display, ftom};

mod osc;
mod spectrum;
mod tracker;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

pub const RING_SIZE: usize = 4096;
pub const DISPLAY_SAMPLES: usize = 512;
pub const DEFAULT_STEPS: usize = 64;

// ---------------------------------------------------------------------------
// Visualizer toggle
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum VisMode {
    Both,
    OscOnly,
    SpecOnly,
    Hidden,
}

impl VisMode {
    fn cycle(self) -> Self {
        match self {
            VisMode::Both => VisMode::OscOnly,
            VisMode::OscOnly => VisMode::SpecOnly,
            VisMode::SpecOnly => VisMode::Hidden,
            VisMode::Hidden => VisMode::Both,
        }
    }

    fn label(self) -> &'static str {
        match self {
            VisMode::Both => "OSC+SPEC",
            VisMode::OscOnly => "OSC",
            VisMode::SpecOnly => "SPEC",
            VisMode::Hidden => "OFF",
        }
    }

    /// Height (in terminal rows) that the visualizer panel should occupy.
    fn height(self) -> u16 {
        match self {
            VisMode::Hidden => 0,
            _ => 10,
        }
    }
}

// ---------------------------------------------------------------------------
// App
// ---------------------------------------------------------------------------

struct App {
    // Audio visualizer pipeline
    consumer: rtrb::Consumer<f32>,
    visualization_ring: VecDeque<f32>,
    oscilloscope: Oscilloscope,
    spectroscope: Spectroscope,

    // Legato backend handle
    frontend: LegatoFrontend,

    // Tracker UI state
    tracker: Tracker,
    vis_mode: VisMode,
}

impl App {
    pub fn new(consumer: rtrb::Consumer<f32>, frontend: LegatoFrontend) -> Self {
        Self {
            consumer,
            visualization_ring: VecDeque::from(vec![0.0; RING_SIZE]),
            oscilloscope: Oscilloscope::default(),
            spectroscope: Spectroscope::new(),
            frontend,
            tracker: Tracker::new(DEFAULT_STEPS),
            vis_mode: VisMode::Both,
        }
    }

    /// Draw a frame.
    pub fn run(&mut self, terminal: &mut DefaultTerminal) -> std::io::Result<()> {
        terminal.draw(|frame| {
            frame.render_widget(self, frame.area());
        })?;
        Ok(())
    }

    /// Push the current state of step `index` to the audio backend.
    pub fn sync_step(&mut self, index: usize) {
        let step = &self.tracker.steps[index];
        let _ = self.frontend.send_node_msg(
            "sequencer",
            legato::msg::NodeMessage::SetStep(StepPayload {
                index,
                freq: Some(step.freq),
                vel: Some(step.vel),
                gate: Some(step.gate),
                length: Some(step.length),
            }),
        );
    }

    /// Push all steps to the backend (called once on startup).
    pub fn sync_all_steps(&mut self) {
        for i in 0..self.tracker.steps.len() {
            self.sync_step(i);
        }
    }
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

impl Widget for &mut App {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Drain the lock-free audio ring into our visualizer buffer.
        let available = self.consumer.slots();
        for _ in 0..available {
            if let Ok(s) = self.consumer.pop() {
                self.visualization_ring.pop_front();
                self.visualization_ring.push_back(s);
            }
        }
        let samples = self.visualization_ring.make_contiguous();
        self.oscilloscope.update(samples);
        self.spectroscope.update(samples);

        // --- Outer layout: tracker | visualizers | help bar ---
        let vis_h = self.vis_mode.height();
        let help_h = 1_u16;

        let [tracker_area, vis_area, help_area] = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(4),
                Constraint::Length(vis_h),
                Constraint::Length(help_h),
            ])
            .areas(area);

        // --- Tracker ---
        self.tracker.render(tracker_area, buf);

        // --- Visualizers ---
        if self.vis_mode != VisMode::Hidden {
            match self.vis_mode {
                VisMode::Both => {
                    let [osc_area, spec_area] = Layout::default()
                        .direction(Direction::Horizontal)
                        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                        .areas(vis_area);
                    self.oscilloscope.render_ref(osc_area, buf);
                    self.spectroscope.render_ref(spec_area, buf);
                }
                VisMode::OscOnly => self.oscilloscope.render_ref(vis_area, buf),
                VisMode::SpecOnly => self.spectroscope.render_ref(vis_area, buf),
                VisMode::Hidden => {}
            }
        }

        // --- Help / status bar ---
        let step = self.tracker.current_step();
        let note = freq_to_note_display(step.freq);
        let midi = ftom(step.freq);
        let gate = if step.gate > 0.5 { "ON" } else { "OFF" };
        let vel = (step.vel * 127.0).round() as u8;
        let col_name = match self.tracker.cursor_col {
            Column::Note => "NOTE",
            Column::Vel => "VEL",
            Column::Gate => "GATE",
            Column::Len => "LEN",
        };

        let help = Line::from(vec![
            Span::styled(" ↑↓", Style::default().fg(Color::Cyan)),
            Span::raw(":row "),
            Span::styled("←→", Style::default().fg(Color::Cyan)),
            Span::raw(":col "),
            Span::styled("+/-", Style::default().fg(Color::Cyan)),
            Span::raw(":edit "),
            Span::styled("[]", Style::default().fg(Color::Cyan)),
            Span::raw(":oct "),
            Span::styled("SPC", Style::default().fg(Color::Cyan)),
            Span::raw(":gate "),
            Span::styled("V", Style::default().fg(Color::Cyan)),
            Span::raw(format!(":vis[{}] ", self.vis_mode.label())),
            Span::styled("Q", Style::default().fg(Color::Red)),
            Span::raw(":quit  "),
            Span::styled("│ ", Style::default().fg(Color::DarkGray)),
            // Current step info
            Span::styled(
                format!(
                    "row {:02X}  col {}  {} (MIDI {:3})  {:.1}Hz  vel {:3}  gate {}  len {:.2}",
                    self.tracker.cursor_row,
                    col_name,
                    note,
                    midi,
                    step.freq,
                    vel,
                    gate,
                    step.length,
                ),
                Style::default().fg(Color::Yellow),
            ),
        ]);

        Paragraph::new(help).render(help_area, buf);
    }
}

// ---------------------------------------------------------------------------
// Legato setup (unchanged from original)
// ---------------------------------------------------------------------------

fn setup_legato_runtime(producer: rtrb::Producer<f32>) -> LegatoFrontend {
    let graph = fs::read_to_string("../.legato").expect("Could not find legato file!");

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

    let (backend, frontend) = LegatoBuilder::<Unconfigured>::new(config, ports)
        .set_midi_runtime(midi_rt_fe)
        .build_dsl(&graph);

    let interface = AudioInterface::default_with_config(&config);

    std::thread::spawn(move || {
        start_application_audio_thread_external_output(interface, producer, backend)
            .expect("Audio thread panic!")
    });

    frontend
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (prod, consumer) = rtrb::RingBuffer::new(48_000);
    let frontend = setup_legato_runtime(prod);

    let mut app = App::new(consumer, frontend);
    // Push default step state to the backend so it's in sync from the start.
    app.sync_all_steps();

    ratatui::run(|terminal| {
        loop {
            if event::poll(Duration::from_millis(16))? {
                if let Event::Key(key) = event::read()? {
                    // Capture the row *before* any edit (navigation doesn't trigger a sync).
                    let row_before_edit = app.tracker.cursor_row;
                    let mut dirty = false;

                    match key.code {
                        // --- Quit ---
                        KeyCode::Char('q') | KeyCode::Char('Q') => return Ok(()),

                        // --- Visualizer cycle ---
                        KeyCode::Char('v') | KeyCode::Char('V') => {
                            app.vis_mode = app.vis_mode.cycle();
                        }

                        // --- Navigation (no backend sync needed) ---
                        KeyCode::Up => app.tracker.move_up(),
                        KeyCode::Down => app.tracker.move_down(),
                        KeyCode::Left => app.tracker.move_left(),
                        KeyCode::Right => app.tracker.move_right(),

                        // --- Editing ---
                        KeyCode::Char('+') | KeyCode::Char('=') => {
                            dirty = app.tracker.increment_cursor();
                        }
                        KeyCode::Char('-') | KeyCode::Char('_') => {
                            dirty = app.tracker.decrement_cursor();
                        }
                        KeyCode::Char(']') => {
                            dirty = app.tracker.octave_up();
                        }
                        KeyCode::Char('[') => {
                            dirty = app.tracker.octave_down();
                        }
                        KeyCode::Char(' ') => {
                            dirty = app.tracker.toggle_gate();
                        }

                        _ => {}
                    }

                    if dirty {
                        // row_before_edit is still the correct row: edits never
                        // move the cursor, so cursor_row hasn't changed.
                        app.sync_step(row_before_edit);
                    }
                }
            }

            app.run(terminal)?;
        }
    })
}
