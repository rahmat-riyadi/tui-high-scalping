use std::collections::VecDeque;
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::DefaultTerminal;
use tokio::sync::mpsc;

use crate::domain::agg_trades::AggTrades;
use crate::domain::book_ticker::BookTicker;
use crate::domain::engine::Engine;
use crate::domain::executor::Fill;
use crate::domain::order_book::OrderBook;
use crate::domain::pnl::PnlStats;
use crate::ui;
use crate::util::now_ms;

const TAPE_INTERVAL: Duration = Duration::from_millis(16);
const INFO_INTERVAL: Duration = Duration::from_millis(100);
const MAX_HISTORY: usize = 200;

pub struct Channels {
    pub bt_rx: mpsc::UnboundedReceiver<BookTicker>,
    pub at_rx: mpsc::UnboundedReceiver<AggTrades>,
    pub pd_rx: mpsc::UnboundedReceiver<OrderBook>,
    pub fill_rx: mpsc::UnboundedReceiver<Fill>,
}

pub struct AppState {
    pub latest_bt: Option<BookTicker>,
    pub history_bt: VecDeque<BookTicker>,
    pub latest_at: Option<AggTrades>,
    pub history_at: VecDeque<AggTrades>,
    pub order_book: Option<OrderBook>,
    pub pnl: PnlStats,
    pub exec_label: &'static str,
    pub armed: bool,
}

impl AppState {
    fn new() -> Self {
        Self {
            latest_bt: None,
            history_bt: VecDeque::new(),
            latest_at: None,
            history_at: VecDeque::new(),
            order_book: None,
            pnl: PnlStats::new(1_000.0),
            exec_label: "IDLE",
            armed: false,
        }
    }
}

pub fn run(
    terminal: &mut DefaultTerminal,
    rx: &mut Channels,
    mut engine: Engine,
) -> std::io::Result<()> {
    let mut state = AppState::new();

    let mut last_push_bt = Instant::now();
    let mut last_push_at = Instant::now();
    let mut last_info = Instant::now();
    let mut dirty = true;

    loop {
        while let Ok(bt) = rx.bt_rx.try_recv() {
            engine.on_book_ticker(&bt, now_ms());
            if last_push_bt.elapsed() >= TAPE_INTERVAL {
                state.history_bt.push_front(bt.clone());
                state.history_bt.truncate(MAX_HISTORY);
                last_push_bt = Instant::now();
            }
            state.latest_bt = Some(bt);
            dirty = true;
        }

        while let Ok(at) = rx.at_rx.try_recv() {
            engine.on_trade(&at, now_ms());
            if last_push_at.elapsed() >= TAPE_INTERVAL {
                state.history_at.push_front(at.clone());
                state.history_at.truncate(MAX_HISTORY);
                last_push_at = Instant::now();
            }
            state.latest_at = Some(at);
            dirty = true;
        }

        while let Ok(book) = rx.pd_rx.try_recv() {
            engine.on_depth(&book, now_ms());
            state.order_book = Some(book);
            dirty = true;
        }

        while let Ok(fill) = rx.fill_rx.try_recv() {
            engine.on_fill(fill, now_ms());
            dirty = true;
        }

        engine.on_tick(now_ms());
        for pnl in engine.take_closed_pnl() {
            state.pnl.push_trade(pnl);
            dirty = true;
        }

        if last_info.elapsed() >= INFO_INTERVAL {
            last_info = Instant::now();
            dirty = true;
        }

        if dirty {
            let snap = engine.ui_snapshot(now_ms());
            state.exec_label = engine.state_label();
            state.armed = engine.armed();
            terminal.draw(|frame| ui::render(frame, &state, snap.as_ref()))?;
            dirty = false;
        }

        if event::poll(Duration::from_millis(16))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Char('q') => return Ok(()),
                        // Saklar keamanan: 't' untuk mengaktifkan/menonaktifkan entry baru.
                        KeyCode::Char('t') => {
                            engine.toggle_armed();
                            dirty = true;
                        }
                        _ => {}
                    }
                }
            }
        }
    }
}