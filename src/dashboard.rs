use std::collections::VecDeque;
use std::io::stdout;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table},
    Frame, Terminal,
};

pub struct PlayerInfo {
    pub id:      String,
    pub addr:    String,
    pub x:       f32,
    pub y:       f32,
    pub strikes: u8,
}

pub enum EventKind { Info, Warn }

pub struct LogEvent {
    pub time:    String,
    pub kind:    EventKind,
    pub message: String,
}

pub struct DashboardData {
    pub tick:      u64,
    pub tick_rate: f64,
    pub players:   Vec<PlayerInfo>,
    pub pending:   usize,
    pub events:    VecDeque<LogEvent>,
    pub uptime:    Instant,
    last_update:   Instant,
    last_tick:     u64,
}

impl DashboardData {
    pub fn new() -> Self {
        let now = Instant::now();
        Self {
            tick: 0, tick_rate: 0.0,
            players: vec![], pending: 0,
            events: VecDeque::new(),
            uptime: now, last_update: now, last_tick: 0,
        }
    }

    pub fn update(&mut self, tick: u64, players: Vec<PlayerInfo>, pending: usize) {
        let elapsed = self.last_update.elapsed().as_secs_f64();
        if elapsed > 0.0 {
            self.tick_rate = (tick.saturating_sub(self.last_tick)) as f64 / elapsed;
        }
        self.last_update = Instant::now();
        self.last_tick = tick;
        self.tick = tick;
        self.players = players;
        self.pending = pending;
    }

    pub fn push_event(&mut self, kind: EventKind, message: String) {
        let now = wall_time();
        self.events.push_front(LogEvent { time: now, kind, message });
        if self.events.len() > 30 {
            self.events.pop_back();
        }
    }
}

fn wall_time() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let s = secs % 60;
    let m = (secs / 60) % 60;
    let h = (secs / 3600) % 24;
    format!("{h:02}:{m:02}:{s:02}")
}

fn fmt_uptime(start: Instant) -> String {
    let secs = start.elapsed().as_secs();
    let s = secs % 60;
    let m = (secs / 60) % 60;
    let h = secs / 3600;
    format!("{h:02}:{m:02}:{s:02}")
}

fn render(f: &mut Frame, data: &DashboardData) {
    let area = f.area();

    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(12),
        ])
        .split(area);

    // ── Header ────────────────────────────────────────────────────────────────
    let header_text = Line::from(vec![
        Span::styled("AEGIS-LINK", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::raw("  ·  port 8080"),
        Span::raw("               uptime  "),
        Span::styled(fmt_uptime(data.uptime), Style::default().fg(Color::Yellow)),
    ]);
    let header = Paragraph::new(header_text)
        .block(Block::default().borders(Borders::ALL));
    f.render_widget(header, outer[0]);

    // ── Body: stats (left) + player table (right) ─────────────────────────────
    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(20), Constraint::Min(0)])
        .split(outer[1]);

    let stats_text = vec![
        Line::from(format!("Tick    {:>6}", data.tick)),
        Line::from(format!("FPS     {:>6.1}", data.tick_rate)),
        Line::from(format!("Auth'd  {:>6}", data.players.len())),
        Line::from(format!("Pending {:>6}", data.pending)),
    ];
    let stats = Paragraph::new(stats_text)
        .block(Block::default().title("STATS").borders(Borders::ALL));
    f.render_widget(stats, body[0]);

    let header_cells = ["UUID", "ADDR", "X", "Y", "Str"]
        .iter()
        .map(|h| Cell::from(*h).style(Style::default().add_modifier(Modifier::BOLD)));
    let header_row = Row::new(header_cells).height(1);

    let rows: Vec<Row> = data.players.iter().map(|p| {
        Row::new(vec![
            Cell::from(p.id.chars().take(8).collect::<String>()),
            Cell::from(p.addr.clone()),
            Cell::from(format!("{:.1}", p.x)),
            Cell::from(format!("{:.1}", p.y)),
            Cell::from(format!("{}", p.strikes)),
        ])
    }).collect();

    let player_table = Table::new(
        rows,
        [
            Constraint::Length(10),
            Constraint::Length(22),
            Constraint::Length(7),
            Constraint::Length(7),
            Constraint::Length(4),
        ],
    )
    .header(header_row)
    .block(Block::default().title("PLAYERS").borders(Borders::ALL));
    f.render_widget(player_table, body[1]);

    // ── Event log ─────────────────────────────────────────────────────────────
    let log_lines: Vec<Line> = data.events.iter().map(|ev| {
        let (label, color) = match ev.kind {
            EventKind::Info => ("INFO", Color::Green),
            EventKind::Warn => ("WARN", Color::Yellow),
        };
        Line::from(vec![
            Span::raw(format!("{}  ", ev.time)),
            Span::styled(format!("{:4}  ", label), Style::default().fg(color)),
            Span::raw(ev.message.clone()),
        ])
    }).collect();

    let event_log = Paragraph::new(log_lines)
        .block(Block::default().title("EVENT LOG").borders(Borders::ALL));
    f.render_widget(event_log, outer[2]);
}

pub fn run_dashboard(data: Arc<Mutex<DashboardData>>) {
    if let Err(e) = run_inner(data) {
        eprintln!("dashboard error: {e}");
    }
}

fn run_inner(data: Arc<Mutex<DashboardData>>) -> std::io::Result<()> {
    enable_raw_mode()?;
    let mut out = stdout();
    execute!(out, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(out);
    let mut terminal = Terminal::new(backend)?;

    loop {
        let snapshot = {
            let d = data.lock().unwrap();
            DashboardData {
                tick:        d.tick,
                tick_rate:   d.tick_rate,
                players:     d.players.iter().map(|p| PlayerInfo {
                    id:      p.id.clone(),
                    addr:    p.addr.clone(),
                    x:       p.x,
                    y:       p.y,
                    strikes: p.strikes,
                }).collect(),
                pending:     d.pending,
                events:      d.events.iter().map(|e| LogEvent {
                    time:    e.time.clone(),
                    kind:    match e.kind {
                        EventKind::Info => EventKind::Info,
                        EventKind::Warn => EventKind::Warn,
                    },
                    message: e.message.clone(),
                }).collect(),
                uptime:      d.uptime,
                last_update: d.last_update,
                last_tick:   d.last_tick,
            }
        };

        terminal.draw(|f| render(f, &snapshot))?;

        if event::poll(Duration::from_millis(200))? {
            if let Event::Key(key) = event::read()? {
                if key.code == KeyCode::Char('q') || key.code == KeyCode::Char('Q') {
                    break;
                }
            }
        }
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    Ok(())
}
