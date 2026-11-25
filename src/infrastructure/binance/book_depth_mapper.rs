use crate::domain::order_book::{OrderBook, PriceLevel};
use binance_sdk::derivatives_trading_usds_futures::websocket_streams::PartialBookDepthStreamsResponse;

fn parse_levels(levels: &[Vec<String>]) -> Vec<PriceLevel> {
  levels
    .iter()
    .filter_map(|level| {
      Some(PriceLevel {
        price: level.get(0)?.parse::<f64>().ok()?,
        qty: level.get(1)?.parse::<f64>().ok()?,
      })
    })
    .collect()
}

pub fn from_response(res: &PartialBookDepthStreamsResponse) -> Option<OrderBook> {
  let bid = parse_levels(res.b.as_ref()?);
  let ask = parse_levels(res.a.as_ref()?);
  Some(OrderBook { bid, ask })
}