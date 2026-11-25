//! Engine: Signals + Executor + gateway dalam satu objek tanpa dependensi UI.
//!
//! `app.rs` cukup memanggil `on_*` untuk setiap event dan `on_tick` tiap iterasi
//! loop. Karena tidak bergantung pada ratatui/tokio, engine ini nanti bisa
//! dipindah ke task/thread sendiri tanpa mengubah logikanya.

use crate::domain::agg_trades::AggTrades;
use crate::domain::book_ticker::BookTicker;
use crate::domain::executor::{
    BinanceGateway, ExecConfig, Executor, Fill, Gateway, PaperGateway, Side,
};
use crate::domain::order_book::OrderBook;
use crate::domain::signal::{SignalConfig, Signals, Snapshot};

pub enum EngineGateway {
    Paper(PaperGateway),
    Live(BinanceGateway),
}

impl Gateway for EngineGateway {
    fn post_only(&mut self, side: Side, price: f64, qty: f64, reduce_only: bool, now: i64) {
        match self {
            EngineGateway::Paper(gw) => gw.post_only(side, price, qty, reduce_only, now),
            EngineGateway::Live(gw) => gw.post_only(side, price, qty, reduce_only, now),
        }
    }

    fn market(&mut self, side: Side, qty: f64, reduce_only: bool, now: i64) {
        match self {
            EngineGateway::Paper(gw) => gw.market(side, qty, reduce_only, now),
            EngineGateway::Live(gw) => gw.market(side, qty, reduce_only, now),
        }
    }

    fn cancel_open(&mut self, now: i64) {
        match self {
            EngineGateway::Paper(gw) => gw.cancel_open(now),
            EngineGateway::Live(gw) => gw.cancel_open(now),
        }
    }
}

pub struct Engine {
    signals: Signals,
    exec: Executor,
    gw: EngineGateway,
    qty: f64,
    last_snap: Option<Snapshot>,
    last_quote: Option<(f64, f64)>,
    reported: usize,
}

impl Engine {
    pub fn new(sig_cfg: SignalConfig, exec_cfg: ExecConfig, paper_latency_ms: i64) -> Self {
        Self {
            signals: Signals::new(sig_cfg),
            qty: exec_cfg.qty,
            exec: Executor::new(exec_cfg),
            gw: EngineGateway::Paper(PaperGateway::new(paper_latency_ms)),
            last_snap: None,
            last_quote: None,
            reported: 0,
        }
    }

    pub fn new_live(
        sig_cfg: SignalConfig,
        exec_cfg: ExecConfig,
        binance_gw: BinanceGateway,
    ) -> Self {
        Self {
            signals: Signals::new(sig_cfg),
            qty: exec_cfg.qty,
            exec: Executor::new(exec_cfg),
            gw: EngineGateway::Live(binance_gw),
            last_snap: None,
            last_quote: None,
            reported: 0,
        }
    }

    pub fn on_fill(&mut self, f: Fill, now: i64) {
        self.exec.on_fill(f, now);
    }

    pub fn on_book_ticker(&mut self, bt: &BookTicker, now: i64) {
        self.signals.on_book_ticker(bt, now);
        self.last_quote = Some((bt.best_bid_price, bt.best_ask_price));

        // 1) fill dulu (berdasarkan quote terbaru jika Paper mode), 2) baru keputusan baru.
        if let EngineGateway::Paper(ref mut paper) = self.gw {
            for f in paper.on_quote(bt.best_bid_price, bt.best_ask_price, now) {
                self.exec.on_fill(f, now);
            }
        }
        if let Some(s) = self.signals.snapshot(now) {
            self.exec.on_snapshot(&s, now, &mut self.gw);
            self.last_snap = Some(s);
        }
    }

    pub fn on_trade(&mut self, at: &AggTrades, now: i64) {
        self.signals.on_trade(at, now);
    }

    pub fn on_depth(&mut self, book: &OrderBook, now: i64) {
        self.signals.on_depth(book, now);
    }

    pub fn on_tick(&mut self, now: i64) {
        if let EngineGateway::Paper(ref mut paper) = self.gw {
            let Some((bid, ask)) = self.last_quote else { return };
            for f in paper.on_quote(bid, ask, now) {
                self.exec.on_fill(f, now);
            }
        }
        if !self.exec.is_flat() {
            if let Some(s) = &self.last_snap {
                self.exec.on_snapshot(s, now, &mut self.gw);
            }
        }
    }

    /// Snapshot segar untuk UI (feed_age ikut berjalan walau tidak ada event).
    pub fn ui_snapshot(&self, now: i64) -> Option<Snapshot> {
        self.signals.snapshot(now)
    }

    /// PnL bersih (mata uang quote, mis. USDT) dari trade yang baru selesai.
    pub fn take_closed_pnl(&mut self) -> Vec<f64> {
        let out: Vec<f64> = self.exec.trades[self.reported..]
            .iter()
            .map(|t| t.net_bps / 1e4 * t.entry_px * self.qty)
            .collect();
        self.reported = self.exec.trades.len();
        out
    }

    pub fn state_label(&self) -> &'static str {
        self.exec.state_label()
    }

    pub fn armed(&self) -> bool {
        self.exec.armed
    }

    pub fn set_armed(&mut self, on: bool) {
        self.exec.armed = on;
    }

    pub fn toggle_armed(&mut self) {
        self.exec.armed = !self.exec.armed;
    }

    pub fn halted(&self) -> bool {
        self.exec.halted
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Aliran sintetis: tekanan beli kuat -> sinyal long -> entry maker ->
    /// terisi -> exit setelah hold -> trade tercatat. Memakai tipe domain asli.
    #[test]
    fn full_chain_entry_fill_exit_records_trade() {
        let sig = SignalConfig {
            edge_per_score_bps: 10.0, // supaya gate edge lolos di tes ini
            ..SignalConfig::default()
        };
        let mut eng = Engine::new(sig, ExecConfig::default(), 5);
        eng.set_armed(true);

        let book = OrderBook {
            bid: vec![crate::domain::order_book::PriceLevel { price: 100.0, qty: 30.0 }; 20],
            ask: vec![crate::domain::order_book::PriceLevel { price: 100.1, qty: 3.0 }; 20],
        };

        let mut t = 1_000_000i64;
        let mut saw_entry_pending = false;
        let mut last_bt = BookTicker::new(t, 100.0, 10.0, 100.1, 5.0);

        // Fase 1: tekanan beli (bid qty naik, ask qty turun, harga tetap).
        for i in 0..40 {
            let bt = BookTicker::new(t - 2, 100.0, 10.0 + i as f64, 100.1, (5.0 - i as f64 * 0.1).max(0.5));
            eng.on_depth(&book, t);
            eng.on_trade(&AggTrades::new(t, 100.05, 1.0, false), t);
            eng.on_book_ticker(&bt, t);
            eng.on_tick(t);
            last_bt = bt;
            if eng.state_label() == "ENTRY?" {
                saw_entry_pending = true;
                break;
            }
            t += 10;
        }
        assert!(saw_entry_pending, "sinyal long seharusnya memicu entry maker");

        // Fase 2a: order sampai ke exchange dan mulai antre (quote tidak berubah).
        t += 10;
        eng.on_book_ticker(&BookTicker::new(t - 2, last_bt.best_bid_price, last_bt.best_bid_qty, last_bt.best_ask_price, last_bt.best_ask_qty), t);
        eng.on_tick(t);
        assert_eq!(eng.state_label(), "ENTRY?");

        // Fase 2b: ask turun ke level kita (100.0) -> limit buy @100.0 terisi.
        t += 10;
        eng.on_book_ticker(&BookTicker::new(t - 2, 99.9, 10.0, 100.0, 10.0), t);
        eng.on_tick(t);
        assert_eq!(eng.state_label(), "HOLD");

        // Fase 3: lewati hold 100 ms, exit market, lalu konfirmasi fill.
        for _ in 0..15 {
            t += 10;
            eng.on_book_ticker(&BookTicker::new(t - 2, 99.95, 10.0, 100.05, 10.0), t);
            eng.on_tick(t);
        }
        let pnl = eng.take_closed_pnl();
        assert_eq!(pnl.len(), 1, "harus ada tepat satu trade selesai");
        assert!(pnl[0] < 0.0, "exit taker di bid + fee harus rugi pada skenario ini");
        assert!(eng.take_closed_pnl().is_empty(), "trade tidak boleh dilaporkan dua kali");
    }

    #[test]
    fn disarmed_engine_never_enters() {
        let sig = SignalConfig {
            edge_per_score_bps: 10.0,
            ..SignalConfig::default()
        };
        let mut eng = Engine::new(sig, ExecConfig::default(), 5); // armed = false
        let mut t = 1_000_000i64;
        for i in 0..40 {
            eng.on_book_ticker(&BookTicker::new(t - 2, 100.0, 10.0 + i as f64, 100.1, 1.0), t);
            eng.on_tick(t);
            t += 10;
        }
        assert_eq!(eng.state_label(), "IDLE");
        assert!(eng.take_closed_pnl().is_empty());
    }
}