use crate::domain::agg_trades::AggTrades;
use binance_sdk::derivatives_trading_usds_futures::websocket_streams;

pub fn from_response(
  res: &websocket_streams::AggregateTradeStreamsResponse,
) -> Option<AggTrades> {
  Some(AggTrades::new(
    res.t_uppercase?,
    res.p.as_ref()?.parse().ok()?,
    res.q.as_ref()?.parse().ok()?,
    res.m?,
  ))
}