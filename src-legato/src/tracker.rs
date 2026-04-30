use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Rect},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Cell, Row, StatefulWidget, Table, TableState, Widget},
};

/// The backend pre-allocates this many steps; we never go above it.
pub const MAX_STEPS: usize = 256;

// Classic tracker note names: natural notes padded with '-' so every name is 2 chars.
const NOTE_NAMES: &[&str] = &[
    "C-", "C#", "D-", "D#", "E-", "F-", "F#", "G-", "G#", "A-", "A#", "B-",
];

// ---------------------------------------------------------------------------
// Data
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct TrackerStep {
    pub freq: f32,   // Hz
    pub vel: f32,    // 0.0 – 1.0
    pub gate: f32,   // 0.0 (off) | 1.0 (on)
    pub length: f32, // 0.0 – 1.0
}

impl Default for TrackerStep {
    fn default() -> Self {
        Self {
            freq: 261.626, // C4
            vel: 0.787_4,  // ≈ 100 / 127
            gate: 0.0,
            length: 0.5,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Column {
    Note,
    Vel,
    Gate,
    Len,
}

impl Column {
    pub fn next(self) -> Self {
        match self {
            Column::Note => Column::Vel,
            Column::Vel => Column::Gate,
            Column::Gate => Column::Len,
            Column::Len => Column::Note,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            Column::Note => Column::Len,
            Column::Vel => Column::Note,
            Column::Gate => Column::Vel,
            Column::Len => Column::Gate,
        }
    }
}

// ---------------------------------------------------------------------------
// Pitch helpers
// ---------------------------------------------------------------------------

/// Convert frequency (Hz) to the nearest MIDI note number.
pub fn freq_to_midi(freq: f32) -> i32 {
    (12.0 * (freq / 440.0).log2() + 69.0).round() as i32
}

/// Convert a MIDI note number to frequency (Hz).
pub fn midi_to_freq(midi: i32) -> f32 {
    440.0 * 2.0_f32.powf((midi as f32 - 69.0) / 12.0)
}

/// Human-readable note name, e.g. "C-4", "A#3". Width is always 3 chars for
/// octaves 0–9 (the usable MIDI range) and 4 for the rare octave -1 (MIDI 0–11).
pub fn freq_to_note_display(freq: f32) -> String {
    let midi = freq_to_midi(freq).clamp(0, 127);
    let octave = (midi / 12) - 1;
    let idx = (midi % 12) as usize;
    format!("{}{}", NOTE_NAMES[idx], octave)
}

// ---------------------------------------------------------------------------
// Tracker state
// ---------------------------------------------------------------------------

pub struct Tracker {
    /// Full preallocated buffer — always MAX_STEPS long.
    pub steps: Vec<TrackerStep>,
    /// How many steps the sequencer currently plays (1 – MAX_STEPS).
    pub active_steps: usize,
    pub cursor_row: usize,
    pub cursor_col: Column,
    /// Kept in sync with cursor_row so the Table widget scrolls automatically.
    pub table_state: TableState,
    pub bpm: f32,
}

impl Tracker {
    pub fn new(initial_steps: usize) -> Self {
        let active_steps = initial_steps.clamp(1, MAX_STEPS);
        let mut table_state = TableState::default();
        table_state.select(Some(0));
        Self {
            steps: vec![TrackerStep::default(); MAX_STEPS],
            active_steps,
            cursor_row: 0,
            cursor_col: Column::Note,
            table_state,
            bpm: 120.0
        }
    }

    // --- Navigation ---

    pub fn move_up(&mut self) {
        self.cursor_row = if self.cursor_row == 0 {
            self.active_steps - 1
        } else {
            self.cursor_row - 1
        };
        self.table_state.select(Some(self.cursor_row));
    }

    pub fn move_down(&mut self) {
        self.cursor_row = (self.cursor_row + 1) % self.active_steps;
        self.table_state.select(Some(self.cursor_row));
    }

    // --- Step count ---

    /// Add one step. Returns the new count, or None if already at MAX_STEPS.
    pub fn grow(&mut self) -> Option<usize> {
        if self.active_steps >= MAX_STEPS {
            return None;
        }
        self.active_steps += 1;
        Some(self.active_steps)
    }

    /// Remove one step. Returns the new count, or None if already at 1.
    /// Clamps the cursor so it stays inside the active window.
    pub fn shrink(&mut self) -> Option<usize> {
        if self.active_steps <= 1 {
            return None;
        }
        self.active_steps -= 1;
        if self.cursor_row >= self.active_steps {
            self.cursor_row = self.active_steps - 1;
            self.table_state.select(Some(self.cursor_row));
        }
        Some(self.active_steps)
    }

    pub fn move_left(&mut self) {
        self.cursor_col = self.cursor_col.prev();
    }
    pub fn move_right(&mut self) {
        self.cursor_col = self.cursor_col.next();
    }

    // --- Editing (all return true so the caller knows to sync the backend) ---

    /// Nudge the focused cell up by one unit.
    pub fn increment(&mut self) -> bool {
        self.adjust(1)
    }

    /// Nudge the focused cell down by one unit.
    pub fn decrement(&mut self) -> bool {
        self.adjust(-1)
    }

    /// Jump up one octave (only applies when cursor is on the Note column).
    pub fn octave_up(&mut self) -> bool {
        if self.cursor_col != Column::Note {
            return false;
        }
        let step = &mut self.steps[self.cursor_row];
        step.freq = midi_to_freq((freq_to_midi(step.freq) + 12).clamp(0, 127));
        true
    }

    /// Jump down one octave (only applies when cursor is on the Note column).
    pub fn octave_down(&mut self) -> bool {
        if self.cursor_col != Column::Note {
            return false;
        }
        let step = &mut self.steps[self.cursor_row];
        step.freq = midi_to_freq((freq_to_midi(step.freq) - 12).clamp(0, 127));
        true
    }

    /// Toggle the gate of the focused step.
    pub fn toggle_gate(&mut self) -> bool {
        let step = &mut self.steps[self.cursor_row];
        step.gate = if step.gate > 0.5 { 0.0 } else { 1.0 };
        true
    }

    pub fn current_step(&self) -> &TrackerStep {
        &self.steps[self.cursor_row]
    }

    // --- Internal ---

    fn adjust(&mut self, dir: i32) -> bool {
        let step = &mut self.steps[self.cursor_row];
        match self.cursor_col {
            Column::Note => {
                let midi = freq_to_midi(step.freq);
                step.freq = midi_to_freq((midi + dir).clamp(0, 127));
            }
            Column::Vel => {
                // 128 discrete levels (0-127)
                let raw = (step.vel * 127.0).round() as i32;
                step.vel = ((raw + dir).clamp(0, 127) as f32) / 127.0;
            }
            Column::Gate => {
                // +/- both toggle for convenience
                step.gate = if step.gate > 0.5 { 0.0 } else { 1.0 };
            }
            Column::Len => {
                // 32 steps of resolution (0.00 – 1.00 in 1/32 increments)
                let raw = (step.length * 32.0).round() as i32;
                step.length = ((raw + dir).clamp(0, 32) as f32) / 32.0;
            }
        }
        true
    }
}

// ---------------------------------------------------------------------------
// Ratatui widget
// ---------------------------------------------------------------------------

/// Colour palette – tweak here to restyle the whole tracker.
mod palette {
    use ratatui::style::Color;
    pub const HEADER_FG: Color = Color::Yellow;
    pub const CURSOR_FG: Color = Color::Black;
    pub const CURSOR_BG: Color = Color::Cyan;
    pub const ROW_ACTIVE_BG: Color = Color::DarkGray;
    pub const ROW_BEAT_FG: Color = Color::White; // every 4th row
    pub const ROW_NORMAL_FG: Color = Color::DarkGray;
    pub const INDEX_FG: Color = Color::DarkGray;
    pub const INDEX_ACTIVE_FG: Color = Color::Yellow;
    pub const GATE_ON_FG: Color = Color::Green;
    pub const TITLE_FG: Color = Color::Cyan;
}

impl Widget for &mut Tracker {
    fn render(self, area: Rect, buf: &mut Buffer) {
        use palette::*;

        let cursor_row = self.cursor_row;
        let cursor_col = self.cursor_col;

        // --- Header ---
        let header = Row::new([
            Cell::from(" # "),
            Cell::from("NOTE "),
            Cell::from(" VEL"),
            Cell::from(" GATE"),
            Cell::from("  LEN"),
        ])
        .style(
            Style::default()
                .fg(HEADER_FG)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        )
        .height(1);

        // --- Rows ---
        let cursor_style = Style::default()
            .fg(CURSOR_FG)
            .bg(CURSOR_BG)
            .add_modifier(Modifier::BOLD);

        let rows: Vec<Row> = self.steps[..self.active_steps]
            .iter()
            .enumerate()
            .map(|(i, step)| {
                let is_active = i == cursor_row;
                let gate_on = step.gate > 0.5;

                let row_base = if is_active {
                    Style::default().fg(ROW_BEAT_FG).bg(ROW_ACTIVE_BG)
                } else if i % 4 == 0 {
                    Style::default().fg(ROW_BEAT_FG)
                } else {
                    Style::default().fg(ROW_NORMAL_FG)
                };

                // Helper: highlight cell if it's under the cursor, else use row_base.
                let cell = |text: String, col: Column| -> Cell {
                    if is_active && cursor_col == col {
                        Cell::from(text).style(cursor_style)
                    } else {
                        Cell::from(text).style(row_base)
                    }
                };

                // Index cell
                let idx_style = if is_active {
                    Style::default()
                        .fg(INDEX_ACTIVE_FG)
                        .bg(ROW_ACTIVE_BG)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(INDEX_FG)
                };

                // Note
                let note_str = format!("{:<5}", freq_to_note_display(step.freq));

                // Velocity — displayed as two-digit hex (00-7F), classic tracker style.
                let vel_int = (step.vel * 127.0).round() as u8;
                let vel_str = format!(" {:02X} ", vel_int);

                // Gate — prominent ON / faint dots
                let gate_str = if gate_on { "  ON " } else { "  ···" };
                let gate_cell = if is_active && cursor_col == Column::Gate {
                    Cell::from(gate_str).style(cursor_style)
                } else if gate_on {
                    Cell::from(gate_str)
                        .style(Style::default().fg(GATE_ON_FG).add_modifier(Modifier::BOLD))
                } else {
                    Cell::from(gate_str).style(row_base)
                };

                // Length — displayed as a 0.00-1.00 decimal
                let len_str = format!(" {:.2} ", step.length);

                Row::new(vec![
                    Cell::from(format!("{:02X} ", i)).style(idx_style),
                    cell(note_str, Column::Note),
                    cell(vel_str, Column::Vel),
                    gate_cell,
                    cell(len_str, Column::Len),
                ])
                .height(1)
            })
            .collect();

        // --- Table ---
        let widths = [
            Constraint::Length(4), // index
            Constraint::Length(5), // note  (e.g. "C-4  ")
            Constraint::Length(5), // vel   (e.g. " 7F  ")
            Constraint::Length(5), // gate  (e.g. "  ON ")
            Constraint::Length(6), // len   (e.g. " 0.50 ")
        ];

        let table = Table::new(rows, widths)
            .header(header)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::DarkGray))
                    .title(format!(
                        " ▶  SEQUENCER  {}/{} ",
                        self.active_steps, MAX_STEPS
                    ))
                    .title_style(Style::default().fg(TITLE_FG).add_modifier(Modifier::BOLD)),
            )
            // We do per-cell highlighting ourselves; suppress the built-in row highlight.
            .highlight_style(Style::default())
            .highlight_symbol("");

        StatefulWidget::render(table, area, buf, &mut self.table_state);
    }
}
