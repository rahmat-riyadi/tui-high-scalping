#[derive(Clone, Copy, Debug)]
pub struct BookTicker {
    pub transaction_time: i64,
    pub best_bid_price: f64,
    pub best_bid_qty: f64,
    pub best_ask_price: f64,
    pub best_ask_qty: f64,
    pub mid_price: f64,
    pub imbalance: f64,
    pub micro: f64,
    pub spread: f64,
}

impl BookTicker {
    pub fn new(
        transaction_time: i64,
        bid_price: f64,
        bid_qty: f64,
        ask_price: f64,
        ask_qty: f64,
    ) -> Self {
        let mid_price = (bid_price + ask_price) / 2.0;
        let total_qty = bid_qty + ask_qty;

        let imbalance = if total_qty > 0.0 {
            (bid_qty - ask_qty) / total_qty
        } else {
            0.0
        };

        let micro = if total_qty > 0.0 {
            (ask_price * bid_qty + bid_price * ask_qty) / total_qty
        } else {
            mid_price
        };

        Self {
            transaction_time,
            best_bid_price: bid_price,
            best_bid_qty: bid_qty,
            best_ask_price: ask_price,
            best_ask_qty: ask_qty,
            mid_price,
            imbalance,
            micro,
            spread: ask_price - bid_price,
        }
    }
}