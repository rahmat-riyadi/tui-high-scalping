#[derive(Debug, Clone, Default)]
pub struct PriceLevel {
  pub price: f64,
  pub qty: f64,
}

#[derive(Debug, Clone, Default)]
pub struct OrderBook {
  pub bid: Vec<PriceLevel>,
  pub ask: Vec<PriceLevel>,
}

// impl OrderBook {
//   fn update_partial(
//     &mut self,
//     bids: Vec<PriceLevel>,
//     ask: Vec<PriceLevel>,
//   ) {
//     self.bid = bids;
//     self.ask = ask;
//   }
// }
