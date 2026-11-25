#[derive(Clone, Copy, Debug)]
pub struct AggTrades {
  pub trade_time: i64,
  pub price: f64,
  pub qty: f64,
  pub maker: bool,
}

impl AggTrades {
  pub fn new(
    trade_time: i64,
    price: f64,
    qty: f64,
    maker: bool,
  ) -> Self {
    Self {
      trade_time,
      price,
      qty,
      maker,
    }
  }
}
