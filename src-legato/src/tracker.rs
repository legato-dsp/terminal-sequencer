use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Rect},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Cell, Row, StatefulWidget, Table, TableState, Widget},
};

// Classic tracker note names: natural notes padded with '-' so every name is 2 chars.
const NOTE_NAMES: &[&str] = &[
    "C-", "C#", "D-", "D#", "E-", "F-", "F#", "G-", "G#", "A-", "A#", "B-",
];

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

pub fn ftom(freq: f32) -> i32 {
    (12.0 * (freq / 440.0).log2() + 69.0).round() as i32
}

pub fn mtof(midi: i32) -> f32 {
    440.0 * 2.0_f32.powf((midi as f32 - 69.0) / 12.0)
}

pub fn freq_to_note_display(freq: f32) -> String {
    let midi = ftom(freq).clamp(0, 127);
    let octave = (midi / 12) - 1;
    let idx = (midi % 12) as usize;
    format!("{}{}", NOTE_NAMES[idx], octave)
}

pub struct Tracker {
    pub steps: Vec<TrackerStep>,
    pub cursor_row: usize,
    pub cursor_col: Column,
    pub table_state: TableState,
}

impl Tracker {
    pub fn new(num_steps: usize) -> Self {
        let mut table_state = TableState::default();
        table_state.select(Some(0));
        Self {
            steps: vec![TrackerStep::default(); num_steps],
            cursor_row: 0,
            cursor_col: Column::Note,
            table_state,
        }
    }

    pub fn move_up(&mut self) {
        self.cursor_row = if self.cursor_row == 0 {
            self.steps.len() - 1
        } else {
            self.cursor_row - 1
        };
        self.table_state.select(Some(self.cursor_row));
    }

    pub fn move_down(&mut self) {
        self.cursor_row = (self.cursor_row + 1) % self.steps.len();
        self.table_state.select(Some(self.cursor_row));
    }

    pub fn move_left(&mut self) {
        self.cursor_col = self.cursor_col.prev();
    }
    pub fn move_right(&mut self) {
        self.cursor_col = self.cursor_col.next();
    }

    pub fn increment_cursor(&mut self) -> bool {
        self.adjust(1)
    }

    pub fn decrement_cursor(&mut self) -> bool {
        self.adjust(-1)
    }

    /// Jump up one octave (only applies when cursor is on the Note column).
    pub fn octave_up(&mut self) -> bool {
        if self.cursor_col != Column::Note {
            return false;
        }
        let step = &mut self.steps[self.cursor_row];
        step.freq = mtof((ftom(step.freq) + 12).clamp(0, 127));
        true
    }

    /// Jump down one octave (only applies when cursor is on the Note column).
    pub fn octave_down(&mut self) -> bool {
        if self.cursor_col != Column::Note {
            return false;
        }
        let step = &mut self.steps[self.cursor_row];
        step.freq = mtof((ftom(step.freq) - 12).clamp(0, 127));
        true
    }

    pub fn toggle_gate(&mut self) -> bool {
        let step = &mut self.steps[self.cursor_row];
        step.gate = if step.gate > 0.5 { 0.0 } else { 1.0 };
        true
    }

    pub fn current_step(&self) -> &TrackerStep {
        &self.steps[self.cursor_row]
    }

    fn adjust(&mut self, dir: i32) -> bool {
        let step = &mut self.steps[self.cursor_row];
        match self.cursor_col {
            Column::Note => {
                let midi = ftom(step.freq);
                step.freq = mtof((midi + dir).clamp(0, 127));
            }
            Column::Vel => {
                let raw = (step.vel * 127.0).round() as i32;
                step.vel = ((raw + dir).clamp(0, 127) as f32) / 127.0;
            }
            Column::Gate => {
                step.gate = if step.gate > 0.5 { 0.0 } else { 1.0 };
            }
            Column::Len => {
                let raw = (step.length * 32.0).round() as i32;
                step.length = ((raw + dir).clamp(0, 32) as f32) / 32.0;
            }
        }
        true
    }
}

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

        let cursor_style = Style::default()
            .fg(CURSOR_FG)
            .bg(CURSOR_BG)
            .add_modifier(Modifier::BOLD);

        let rows: Vec<Row> = self
            .steps
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

                // Highlight cell if it's under the cursor, else use row_base.
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

        let widths = [
            Constraint::Length(4),
            Constraint::Length(5),
            Constraint::Length(5),
            Constraint::Length(5),
            Constraint::Length(6),
        ];

        let table = Table::new(rows, widths)
            .header(header)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::DarkGray))
                    .title(" ▶  SEQUENCER ")
                    .title_style(Style::default().fg(TITLE_FG).add_modifier(Modifier::BOLD)),
            )
            .row_highlight_style(Style::default())
            .highlight_symbol("");

        StatefulWidget::render(table, area, buf, &mut self.table_state);
    }
}
