use crate::domain::signal::{Direction, ExecMode, Snapshot};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Side {
    Buy,
    Sell,
}

impl Side {
    pub fn opposite(self) -> Side {
        match self {
            Side::Buy => Side::Sell,
            Side::Sell => Side::Buy,
        }
    }

    pub fn sign(self) -> f64 {
        match self {
            Side::Buy => 1.0,
            Side::Sell => -1.0,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ExecConfig {
    pub qty: f64,
    pub hold_ms: i64,
    pub entry_timeout_ms: i64,
    pub passive_exit_ms: i64,
    pub cooldown_ms: i64,
    pub allow_taker_entry: bool,
    pub fee_maker_bps: f64,
    pub fee_taker_bps: f64,
    pub max_exit_wait_ms: i64,
}

impl Default for ExecConfig {
    fn default() -> Self {
        Self {
            qty: 0.001,
            hold_ms: 5_000,
            entry_timeout_ms: 300,
            passive_exit_ms: 0,
            cooldown_ms: 500,
            allow_taker_entry: false,
            fee_maker_bps: 2.0,
            fee_taker_bps: 5.0,
            max_exit_wait_ms: 3_000,
        }
    }
}

pub trait Gateway {
    fn post_only(&mut self, side: Side, price: f64, qty: f64, reduce_only: bool, now: i64);
    fn market(&mut self, side: Side, qty: f64, reduce_only: bool, now: i64);
    /// Batalkan semua order limit yang masih terbuka/in-flight.
    fn cancel_open(&mut self, now: i64);
}

#[derive(Clone, Debug)]
pub enum OrderCommand {
    PostOnly {
        side: Side,
        price: f64,
        qty: f64,
        reduce_only: bool,
        now: i64,
    },
    Market {
        side: Side,
        qty: f64,
        reduce_only: bool,
        now: i64,
    },
    CancelAll {
        now: i64,
    },
}

#[derive(Clone)]
pub struct BinanceGateway {
    tx: tokio::sync::mpsc::UnboundedSender<OrderCommand>,
}

impl BinanceGateway {
    pub fn new(tx: tokio::sync::mpsc::UnboundedSender<OrderCommand>) -> Self {
        Self { tx }
    }
}

impl Gateway for BinanceGateway {
    fn post_only(&mut self, side: Side, price: f64, qty: f64, reduce_only: bool, now: i64) {
        let _ = self.tx.send(OrderCommand::PostOnly {
            side,
            price,
            qty,
            reduce_only,
            now,
        });
    }

    fn market(&mut self, side: Side, qty: f64, reduce_only: bool, now: i64) {
        let _ = self.tx.send(OrderCommand::Market {
            side,
            qty,
            reduce_only,
            now,
        });
    }

    fn cancel_open(&mut self, now: i64) {
        let _ = self.tx.send(OrderCommand::CancelAll { now });
    }
}

/// Konfirmasi fill dari exchange (user data stream / respons order).
#[derive(Clone, Copy, Debug)]
pub enum Fill {
    Entry {
        side: Side,
        price: f64,
        maker: bool,
        ts: i64,
    },
    Exit {
        price: f64,
        maker: bool,
        ts: i64,
    },
}

#[derive(Clone, Copy, Debug)]
pub struct TradeRecord {
    pub side: Side,
    pub entry_px: f64,
    pub exit_px: f64,
    pub held_ms: i64,
    pub gross_bps: f64,
    pub fee_bps: f64,
    pub net_bps: f64,
}

#[derive(Clone, Copy, Debug)]
enum State {
    Idle,
    EntryPending {
        side: Side,
        sent: i64,
    },
    InPosition {
        side: Side,
        entry_px: f64,
        entry_maker: bool,
        since: i64,
    },
    ExitPending {
        side: Side,
        entry_px: f64,
        entry_maker: bool,
        since: i64,
        sent: i64,
        passive: bool,
    },
}

pub struct Executor {
    cfg: ExecConfig,
    state: State,
    cooldown_until: i64,
    /// Saklar keamanan: false = tidak membuka posisi baru (posisi berjalan tetap diselesaikan).
    pub armed: bool,
    pub halted: bool,
    pub trades: Vec<TradeRecord>,
}

impl Executor {
    pub fn new(cfg: ExecConfig) -> Self {
        Self {
            cfg,
            state: State::Idle,
            cooldown_until: 0,
            armed: false,
            halted: false,
            trades: Vec::new(),
        }
    }

    pub fn is_flat(&self) -> bool {
        matches!(self.state, State::Idle)
    }

    /// Label singkat untuk TUI.
    pub fn state_label(&self) -> &'static str {
        match self.state {
            State::Idle => "IDLE",
            State::EntryPending { .. } => "ENTRY?",
            State::InPosition { .. } => "HOLD",
            State::ExitPending { .. } => "EXIT?",
        }
    }

    /// Panggil setiap kali ada Snapshot baru.
    pub fn on_snapshot<G: Gateway>(&mut self, s: &Snapshot, now: i64, gw: &mut G) {
        match self.state {
            State::Idle => {
                if !self.armed || self.halted || now < self.cooldown_until || !s.gates.all() {
                    return;
                }
                let side = match s.direction {
                    Direction::Long => Side::Buy,
                    Direction::Short => Side::Sell,
                    Direction::Flat => return,
                };
                match s.exec_mode {
                    ExecMode::Maker => {
                        // Pasang di sisi kita: long di best bid, short di best ask.
                        let px = if side == Side::Buy { s.bid } else { s.ask };
                        gw.post_only(side, px, self.cfg.qty, false, now);
                        self.state = State::EntryPending { side, sent: now };
                    }
                    ExecMode::Taker if self.cfg.allow_taker_entry => {
                        gw.market(side, self.cfg.qty, false, now);
                        self.state = State::EntryPending { side, sent: now };
                    }
                    _ => {}
                }
            }

            State::EntryPending { side, sent } => {
                let still_wanted = match s.direction {
                    Direction::Long => side == Side::Buy,
                    Direction::Short => side == Side::Sell,
                    Direction::Flat => false,
                };
                if now - sent >= self.cfg.entry_timeout_ms || !still_wanted {
                    gw.cancel_open(now);
                    self.state = State::Idle;
                    self.cooldown_until = now + self.cfg.cooldown_ms;
                    // Jika fill sempat terjadi sebelum cancel sampai, `on_fill`
                    // akan menangkapnya dari state Idle (orphan fill).
                }
            }

            State::InPosition {
                side,
                entry_px,
                entry_maker,
                since,
            } => {
                if now - since < self.cfg.hold_ms {
                    return;
                }
                let exit_side = side.opposite();
                let passive = self.cfg.passive_exit_ms > 0;
                if passive {
                    // Exit long = jual di best ask; exit short = beli di best bid.
                    let px = if exit_side == Side::Sell {
                        s.ask
                    } else {
                        s.bid
                    };
                    gw.post_only(exit_side, px, self.cfg.qty, true, now);
                } else {
                    gw.market(exit_side, self.cfg.qty, true, now);
                }
                self.state = State::ExitPending {
                    side,
                    entry_px,
                    entry_maker,
                    since,
                    sent: now,
                    passive,
                };
            }

            State::ExitPending {
                side,
                entry_px,
                entry_maker,
                since,
                sent,
                passive,
            } => {
                if passive && now - sent >= self.cfg.passive_exit_ms {
                    gw.cancel_open(now);
                    gw.market(side.opposite(), self.cfg.qty, true, now);
                    self.state = State::ExitPending {
                        side,
                        entry_px,
                        entry_maker,
                        since,
                        sent: now,
                        passive: false,
                    };
                } else if !passive && now - sent >= self.cfg.max_exit_wait_ms {
                    self.halted = true;
                }
            }
        }
    }

    pub fn on_fill(&mut self, f: Fill, now: i64) {
        match (f, self.state) {
            (
                Fill::Entry {
                    side,
                    price,
                    maker,
                    ts,
                },
                State::EntryPending { .. } | State::Idle,
            ) => {
                self.state = State::InPosition {
                    side,
                    entry_px: price,
                    entry_maker: maker,
                    since: ts,
                };
            }
            (
                Fill::Exit { price, maker, ts },
                State::ExitPending {
                    side,
                    entry_px,
                    entry_maker,
                    since,
                    ..
                },
            ) => {
                let gross_bps = side.sign() * (price - entry_px) / entry_px * 1e4;
                let fee = |m: bool| {
                    if m {
                        self.cfg.fee_maker_bps
                    } else {
                        self.cfg.fee_taker_bps
                    }
                };
                let fee_bps = fee(entry_maker) + fee(maker);
                self.trades.push(TradeRecord {
                    side,
                    entry_px,
                    exit_px: price,
                    held_ms: ts - since,
                    gross_bps,
                    fee_bps,
                    net_bps: gross_bps - fee_bps,
                });
                self.state = State::Idle;
                self.cooldown_until = now + self.cfg.cooldown_ms;
            }
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// PaperGateway: simulasi eksekusi dengan latensi, untuk uji tanpa uang.
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
enum Kind {
    Post { price: f64 },
    Market,
}

#[derive(Clone, Copy, Debug)]
struct PaperOrder {
    side: Side,
    kind: Kind,
    qty: f64,
    reduce_only: bool,
    active_at: i64,
    cancel_at: Option<i64>,
    /// (bid, ask) saat order mulai aktif di exchange; dipakai untuk menilai
    /// posisi antrean.
    armed: Option<(f64, f64)>,
}

/// Simulator konservatif:
/// - order baru baru aktif setelah `latency_ms` (begitu juga cancel),
/// - post-only ditolak jika menyeberang spread saat aktif,
/// - limit terisi hanya jika harga menembus level kita (tanpa model antrean),
/// - market terisi di sisi lawan pada quote berikutnya setelah aktif.
pub struct PaperGateway {
    latency_ms: i64,
    orders: Vec<PaperOrder>,
}

impl PaperGateway {
    pub fn new(latency_ms: i64) -> Self {
        Self {
            latency_ms,
            orders: Vec::new(),
        }
    }

    pub fn open_orders(&self) -> usize {
        self.orders.len()
    }

    fn mk_fill(o: &PaperOrder, price: f64, maker: bool, ts: i64) -> Fill {
        if o.reduce_only {
            Fill::Exit { price, maker, ts }
        } else {
            Fill::Entry {
                side: o.side,
                price,
                maker,
                ts,
            }
        }
    }

    /// Majukan simulasi dengan quote terbaru; kembalikan fill yang terjadi.
    /// Teruskan hasilnya ke `Executor::on_fill`.
    pub fn on_quote(&mut self, bid: f64, ask: f64, now: i64) -> Vec<Fill> {
        let mut fills = Vec::new();
        let mut keep = Vec::with_capacity(self.orders.len());

        for mut o in self.orders.drain(..) {
            let is_post = matches!(o.kind, Kind::Post { .. });
            if is_post && o.cancel_at.map_or(false, |c| c <= now) {
                continue; // cancel sudah sampai ke exchange
            }
            if o.active_at > now {
                keep.push(o);
                continue;
            }
            match o.kind {
                Kind::Market => {
                    let px = if o.side == Side::Buy { ask } else { bid };
                    fills.push(Self::mk_fill(&o, px, false, now));
                }
                Kind::Post { price } => match o.armed {
                    None => {
                        let crosses = match o.side {
                            Side::Buy => price >= ask,
                            Side::Sell => price <= bid,
                        };
                        if crosses {
                            continue; // post-only ditolak
                        }
                        o.armed = Some((bid, ask));
                        keep.push(o);
                    }
                    Some((abid, aask)) => {
                        let filled = match o.side {
                            Side::Buy => ask <= price || (bid < price && abid >= price),
                            Side::Sell => bid >= price || (ask > price && aask <= price),
                        };
                        if filled {
                            fills.push(Self::mk_fill(&o, price, true, now));
                        } else {
                            keep.push(o);
                        }
                    }
                },
            }
        }
        self.orders = keep;
        fills
    }
}

impl Gateway for PaperGateway {
    fn post_only(&mut self, side: Side, price: f64, qty: f64, reduce_only: bool, now: i64) {
        self.orders.push(PaperOrder {
            side,
            kind: Kind::Post { price },
            qty,
            reduce_only,
            active_at: now + self.latency_ms,
            cancel_at: None,
            armed: None,
        });
    }

    fn market(&mut self, side: Side, qty: f64, reduce_only: bool, now: i64) {
        self.orders.push(PaperOrder {
            side,
            kind: Kind::Market,
            qty,
            reduce_only,
            active_at: now + self.latency_ms,
            cancel_at: None,
            armed: None,
        });
    }

    fn cancel_open(&mut self, now: i64) {
        let at = now + self.latency_ms;
        for o in self.orders.iter_mut() {
            if matches!(o.kind, Kind::Post { .. }) && o.cancel_at.is_none() {
                o.cancel_at = Some(at);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::signal::Gates;

    fn snap(dir: Direction, mode: ExecMode, bid: f64, ask: f64) -> Snapshot {
        Snapshot {
            ts: 0,
            bid,
            ask,
            mid: (bid + ask) / 2.0,
            direction: dir,
            exec_mode: mode,
            score: 0.9,
            micro_ticks: 0.0,
            micro_norm: 0.0,
            imb_l1: 0.0,
            imb_l5: 0.0,
            imb_l20: 0.0,
            ofi_100ms: 0.0,
            ofi_1s: 0.0,
            flow_1s: 0.0,
            trades_per_s: 0.0,
            intensity: 0.0,
            spread_ticks: 1.0,
            spread_bps: 0.1,
            vol_10s_bps: 0.0,
            latency_ms: 0,
            feed_age_ms: 0,
            gross_edge_bps: 0.0,
            cost_bps: 0.0,
            net_edge_bps: 0.0,
            cost_maker_bps: 0.0,
            cost_taker_bps: 0.0,
            net_edge_maker_bps: 0.0,
            net_edge_taker_bps: 0.0,
            gates: Gates {
                signal: true,
                spread: true,
                edge: true,
                latency: true,
            },
        }
    }

    fn armed_exec() -> Executor {
        let mut ex = Executor::new(ExecConfig::default());
        ex.armed = true;
        ex
    }

    fn step(ex: &mut Executor, gw: &mut PaperGateway, t: i64, bid: f64, ask: f64, s: &Snapshot) {
        for f in gw.on_quote(bid, ask, t) {
            ex.on_fill(f, t);
        }
        ex.on_snapshot(s, t, gw);
    }

    #[test]
    fn maker_entry_then_market_exit_after_hold() {
        let mut ex = armed_exec();
        let mut gw = PaperGateway::new(5);
        let long = snap(Direction::Long, ExecMode::Maker, 100.0, 100.1);

        step(&mut ex, &mut gw, 0, 100.0, 100.1, &long); // kirim buy post-only @100.0
        step(&mut ex, &mut gw, 10, 100.0, 100.1, &long); // aktif, antre
        step(&mut ex, &mut gw, 20, 99.9, 100.0, &long); // ask turun ke 100.0 -> terisi
        assert_eq!(ex.state_label(), "HOLD");
        step(&mut ex, &mut gw, 130, 99.95, 100.05, &long); // hold 110 ms >= 100 -> exit market
        step(&mut ex, &mut gw, 140, 99.95, 100.05, &long); // market terisi di bid

        assert_eq!(ex.trades.len(), 1);
        let t = ex.trades[0];
        assert!((t.entry_px - 100.0).abs() < 1e-9);
        assert!((t.exit_px - 99.95).abs() < 1e-9);
        assert!((t.fee_bps - 7.0).abs() < 1e-9); // maker + taker
        assert!(t.net_bps < t.gross_bps);
        assert!(ex.is_flat());
    }

    #[test]
    fn unfilled_entry_is_cancelled_after_timeout() {
        let mut ex = armed_exec();
        let mut gw = PaperGateway::new(5);
        let long = snap(Direction::Long, ExecMode::Maker, 100.0, 100.1);

        step(&mut ex, &mut gw, 0, 100.0, 100.1, &long);
        step(&mut ex, &mut gw, 10, 100.0, 100.1, &long);
        step(&mut ex, &mut gw, 310, 100.0, 100.1, &long); // timeout -> cancel
        assert!(ex.is_flat());
        step(&mut ex, &mut gw, 320, 100.0, 100.1, &long); // cancel sampai
        assert_eq!(gw.open_orders(), 0);
        assert!(ex.trades.is_empty());
    }

    #[test]
    fn late_fill_after_cancel_is_adopted_and_exited() {
        let mut ex = armed_exec();
        let mut gw = PaperGateway::new(50); // latensi tinggi -> cancel lambat
        let long = snap(Direction::Long, ExecMode::Maker, 100.0, 100.1);

        step(&mut ex, &mut gw, 0, 100.0, 100.1, &long);
        step(&mut ex, &mut gw, 60, 100.0, 100.1, &long); // aktif
        step(&mut ex, &mut gw, 310, 100.0, 100.1, &long); // timeout: cancel dikirim (sampai t=360)
        assert!(ex.is_flat());
        step(&mut ex, &mut gw, 330, 99.9, 100.0, &long); // fill terjadi sebelum cancel sampai
        assert_eq!(ex.state_label(), "HOLD"); // orphan fill diadopsi, bukan hilang
    }

    #[test]
    fn no_entry_when_gates_fail() {
        let mut ex = armed_exec();
        let mut gw = PaperGateway::new(5);
        let mut s = snap(Direction::Long, ExecMode::Maker, 100.0, 100.1);
        s.gates.latency = false;
        step(&mut ex, &mut gw, 0, 100.0, 100.1, &s);
        assert!(ex.is_flat());
        assert_eq!(gw.open_orders(), 0);
    }
}
