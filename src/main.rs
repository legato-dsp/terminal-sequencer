use std::error::Error;

use crossterm::event;
use legato::nodes::control::sequencer::SequencerStep;
use ratatui::style::Stylize;
use ratatui::widgets::{Block, Paragraph};

struct SequencerState {
    data: Vec<SequencerStep>,
}

fn start_ui_thread() -> Result<(), Box<dyn Error>> {
    ratatui::run(|terminal| {
        loop {
            terminal.draw(|frame| frame.render_widget("Hello, world!", frame.area()))?;
            if event::read()?.is_key_press() {
                break Ok(());
            }
        }
    })
}

fn main() -> Result<(), Box<dyn Error>> {
    start_ui_thread()
}
