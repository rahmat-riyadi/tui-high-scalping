use std::collections::VecDeque;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::domain::agg_trades::AggTrades;
use crate::domain::book_ticker::BookTicker;
use crate::domain::order_book::OrderBook;

// pub fn now_ms() -> i64 {
//     SystemTime::now()
//         .duration_since(UNIX_EPOCH)
//         .map(|d| d.as_millis() as i64)
//         .unwrap_or(0)
// }

pub struct SignalConfig {
    pub tick_size: f64,
    pub fee_maker_bps: f64,
    pub fee_taker_bps: f64,
    pub edge_per_score_bps: f64,
    pub entry_threshold: f64,
    pub max_spread_ticks: f64,
    pub max_latency_ms: i64,
    pub max_feed_age_ms: i64,
    pub w_micro: f64,
    pub w_imb: f64,
    pub w_ofi: f64,
    pub w_flow: f64,
}

impl Default for SignalConfig {
    fn default() -> Self {
        Self {
            tick_size: 0.1,
            fee_maker_bps: 2.0,
            fee_taker_bps: 5.0,
            edge_per_score_bps: 3.0,
            entry_threshold: 0.5,
            max_spread_ticks: 2.0,
            max_latency_ms: 150,
            max_feed_age_ms: 500,
            w_micro: 0.30,
            w_imb: 0.20,
            w_ofi: 0.30,
            w_flow: 0.20,
        }
    }
}

#[derive(Clone, Copy, Default)]
struct WindowSum {
    signed: f64,
    abs: f64,
    count: usize,
}

impl WindowSum {
    fn ratio(&self) -> f64 {
        if self.abs > 0.0 {
            (self.signed / self.abs).clamp(-1.0, 1.0)
        } else {
            0.0
        }
    }
}

struct RollingWindow {
    buf: VecDeque<(i64, f64, f64)>,
    retain_ms: i64,
}

impl RollingWindow {
    fn new(retain_ms: i64) -> Self {
        Self {
            buf: VecDeque::new(),
            retain_ms,
        }
    }

    fn push(&mut self, t: i64, signed: f64) {
        self.buf.push_back((t, signed, signed.abs()));
        while let Some(&(old, _, _)) = self.buf.front() {
            if t - old > self.retain_ms {
                self.buf.pop_front();
            } else {
                break;
            }
        }
    }

    fn sum(&self, now: i64, win_ms: i64) -> WindowSum {
        self.buf
            .iter()
            .rev()
            .take_while(|e| now - e.0 <= win_ms)
            .fold(WindowSum::default(), |mut s, e| {
                s.signed += e.1;
                s.abs += e.2;
                s.count += 1;
                s
            })
    }
}

#[derive(Clone, Copy)]
struct Quote {
    bid_p: f64,
    bid_q: f64,
    ask_p: f64,
    ask_q: f64,
}

fn ofi_event(p: &Quote, c: &Quote) -> f64 {
    let bid = if c.bid_p > p.bid_p {
        c.bid_q
    } else if c.bid_p < p.bid_p {
        -p.bid_q
    } else {
        c.bid_q - p.bid_q
    };
    let ask = if c.ask_p < p.ask_p {
        c.ask_q
    } else if c.ask_p > p.ask_p {
        -p.ask_q
    } else {
        c.ask_q - p.ask_q
    };
    bid - ask
}

fn depth_imbalance(book: &OrderBook, n: usize) -> f64 {
    let b: f64 = book.bid.iter().take(n).map(|l| l.qty).sum();
    let a: f64 = book.ask.iter().take(n).map(|l| l.qty).sum();
    let t = b + a;
    if t > 0.0 { (b - a) / t } else { 0.0 }
}

pub struct Signals {
    cfg: SignalConfig,
    quote: Option<Quote>,
    ofi: RollingWindow,
    flow: RollingWindow,
    mids: VecDeque<(i64, f64)>,
    last_mid_sample: i64,
    imb_l1: f64,
    imb_l5: f64,
    imb_l20: f64,
    last_bt_recv: i64,
    last_depth_recv: i64,
    latency_ms: i64,
}

impl Signals {
    pub fn new(cfg: SignalConfig) -> Self {
        Self {
            cfg,
            quote: None,
            ofi: RollingWindow::new(5_000),
            flow: RollingWindow::new(10_000),
            mids: VecDeque::new(),
            last_mid_sample: 0,
            imb_l1: 0.0,
            imb_l5: 0.0,
            imb_l20: 0.0,
            last_bt_recv: 0,
            last_depth_recv: 0,
            latency_ms: 0,
        }
    }

    pub fn on_book_ticker(&mut self, bt: &BookTicker, now: i64) {
        let q = Quote {
            bid_p: bt.best_bid_price,
            bid_q: bt.best_bid_qty,
            ask_p: bt.best_ask_price,
            ask_q: bt.best_ask_qty,
        };

        if let Some(prev) = self.quote {
            self.ofi.push(now, ofi_event(&prev, &q));
        }
        self.quote = Some(q);
        self.last_bt_recv = now;
        self.latency_ms = now - bt.transaction_time;

        if now - self.last_mid_sample >= 100 {
            self.mids.push_back((now, (q.bid_p + q.ask_p) / 2.0));
            self.last_mid_sample = now;
            while let Some(&(t, _)) = self.mids.front() {
                if now - t > 10_500 {
                    self.mids.pop_front();
                } else {
                    break;
                }
            }
        }
    }

    pub fn on_trade(&mut self, at: &AggTrades, now: i64) {
        let signed = if at.maker { -at.qty } else { at.qty };
        self.flow.push(now, signed);
    }

    pub fn on_depth(&mut self, book: &OrderBook, now: i64) {
        self.imb_l1 = depth_imbalance(book, 1);
        self.imb_l5 = depth_imbalance(book, 5);
        self.imb_l20 = depth_imbalance(book, 20);
        self.last_depth_recv = now;
    }

    fn realized_vol_bps(&self, now: i64) -> f64 {
        let mut sum = 0.0;
        let mut prev: Option<f64> = None;
        for &(t, m) in &self.mids {
            if now - t > 10_000 {
                continue;
            }
            if let Some(p) = prev {
                if p > 0.0 && m > 0.0 {
                    let r = (m / p).ln();
                    sum += r * r;
                }
            }
            prev = Some(m);
        }
        sum.sqrt() * 1e4
    }

    pub fn snapshot(&self, now: i64) -> Option<Snapshot> {
        let q = self.quote?;
        let c = &self.cfg;

        let mid = (q.bid_p + q.ask_p) / 2.0;
        let spread = q.ask_p - q.bid_p;
        let spread_ticks = spread / c.tick_size;
        let spread_bps = if mid > 0.0 { spread / mid * 1e4 } else { 0.0 };

        let qty_sum = q.bid_q + q.ask_q;
        let micro = if qty_sum > 0.0 {
            (q.ask_p * q.bid_q + q.bid_p * q.ask_q) / qty_sum
        } else {
            mid
        };
        let micro_ticks = (micro - mid) / c.tick_size;
        let micro_norm = if spread > 0.0 {
            ((micro - mid) / (spread / 2.0)).clamp(-1.0, 1.0)
        } else {
            0.0
        };

        let ofi_100ms = self.ofi.sum(now, 100).ratio();
        let ofi_1s = self.ofi.sum(now, 1_000).ratio();
        let flow_1s_sum = self.flow.sum(now, 1_000);
        let flow_1s = flow_1s_sum.ratio();

        let trades_per_s = flow_1s_sum.count as f64;
        let avg_10s = self.flow.sum(now, 10_000).count as f64 / 10.0;
        let intensity = if avg_10s > 0.0 {
            trades_per_s / avg_10s
        } else {
            0.0
        };

        let score = (c.w_micro * micro_norm
            + c.w_imb * (self.imb_l1 + self.imb_l5) / 2.0
            + c.w_ofi * (ofi_100ms + ofi_1s) / 2.0
            + c.w_flow * flow_1s)
            .clamp(-1.0, 1.0);

        let direction = if score >= c.entry_threshold {
            Direction::Long
        } else if score <= -c.entry_threshold {
            Direction::Short
        } else {
            Direction::Flat
        };

        let gross_edge_bps = score.abs() * c.edge_per_score_bps;
        let cost_maker_bps = 2.0 * c.fee_maker_bps;
        let cost_taker_bps = spread_bps + 2.0 * c.fee_taker_bps;
        let net_edge_maker_bps = gross_edge_bps - cost_maker_bps;
        let net_edge_taker_bps = gross_edge_bps - cost_taker_bps;

        let exec_mode = if direction == Direction::Flat {
            ExecMode::Skip
        } else if net_edge_maker_bps > 0.0 {
            ExecMode::Maker
        } else if net_edge_taker_bps > 0.0 {
            ExecMode::Taker
        } else {
            ExecMode::Skip
        };

        let (cost_bps, net_edge_bps) = match exec_mode {
            ExecMode::Taker => (cost_taker_bps, net_edge_taker_bps),
            _ => (cost_maker_bps, net_edge_maker_bps),
        };

        let bt_age = now - self.last_bt_recv;
        let feed_age_ms = if self.last_depth_recv > 0 {
            bt_age.max(now - self.last_depth_recv)
        } else {
            bt_age
        };

        let gates = Gates {
            signal: direction != Direction::Flat,
            spread: spread_ticks <= c.max_spread_ticks,
            edge: exec_mode != ExecMode::Skip,
            latency: self.latency_ms <= c.max_latency_ms && feed_age_ms <= c.max_feed_age_ms,
        };

        Some(Snapshot {
            ts: now,
            bid: q.bid_p,
            ask: q.ask_p,
            mid,
            direction,
            exec_mode,
            score,
            micro_ticks,
            micro_norm,
            imb_l1: self.imb_l1,
            imb_l5: self.imb_l5,
            imb_l20: self.imb_l20,
            ofi_100ms,
            ofi_1s,
            flow_1s,
            trades_per_s,
            intensity,
            spread_ticks,
            spread_bps,
            vol_10s_bps: self.realized_vol_bps(now),
            latency_ms: self.latency_ms,
            feed_age_ms,
            gross_edge_bps,
            cost_bps,
            net_edge_bps,
            cost_maker_bps,
            cost_taker_bps,
            net_edge_maker_bps,
            net_edge_taker_bps,
            gates,
        })
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Long,
    Short,
    Flat,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExecMode {
    Maker,
    Taker,
    Skip,
}

#[derive(Clone, Copy)]
pub struct Gates {
    pub signal: bool,
    pub spread: bool,
    pub edge: bool,
    pub latency: bool,
}

impl Gates {
    pub fn all(&self) -> bool {
        self.signal && self.spread && self.edge && self.latency
    }
}

#[derive(Clone)]
pub struct Snapshot {
    pub ts: i64,
    pub bid: f64,
    pub ask: f64,
    pub mid: f64,
    pub direction: Direction,
    pub exec_mode: ExecMode,
    pub score: f64,
    pub micro_ticks: f64,
    pub micro_norm: f64,
    pub imb_l1: f64,
    pub imb_l5: f64,
    pub imb_l20: f64,
    pub ofi_100ms: f64,
    pub ofi_1s: f64,
    pub flow_1s: f64,
    pub trades_per_s: f64,
    pub intensity: f64,
    pub spread_ticks: f64,
    pub spread_bps: f64,
    pub vol_10s_bps: f64,
    pub latency_ms: i64,
    pub feed_age_ms: i64,
    pub gross_edge_bps: f64,
    pub cost_bps: f64,
    pub net_edge_bps: f64,
    pub cost_maker_bps: f64,
    pub cost_taker_bps: f64,
    pub net_edge_maker_bps: f64,
    pub net_edge_taker_bps: f64,
    pub gates: Gates,
}