pub const MAX_POINTS: usize = 300; // jumlah titik curve yang tampil, chart akan "scroll"

#[derive(Default)]
pub struct PnlStats {
    pub equity: Vec<(f64, f64)>,
    pub total_pnl: f64,
    pub pnl_pct: f64,
    pub trades: u32,
    pub win_rate: f64,
    pub max_drawdown: f64,
    pub current_drawdown: f64,

    wins: u32,
    peak: f64,
    initial_capital: f64,
}

impl PnlStats {
    pub fn new(initial_capital: f64) -> Self {
        Self {
            equity: vec![(0.0, 0.0)],
            initial_capital,
            ..Default::default()
        }
    }

    pub fn push_trade(&mut self, pnl: f64) {
        self.trades += 1;
        if pnl > 0.0 {
            self.wins += 1;
        }

        self.total_pnl += pnl;
        self.pnl_pct = self.total_pnl / self.initial_capital * 100.0;
        self.win_rate = self.wins as f64 / self.trades as f64;

        self.peak = self.peak.max(self.total_pnl);
        self.current_drawdown = self.peak - self.total_pnl;
        self.max_drawdown = self.max_drawdown.max(self.current_drawdown);

        self.equity.push((self.trades as f64, self.total_pnl));
        if self.equity.len() > MAX_POINTS {
            self.equity.remove(0);
        }
    }
}