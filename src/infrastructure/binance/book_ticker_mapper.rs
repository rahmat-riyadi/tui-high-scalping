use crate::domain::book_ticker::BookTicker;
use binance_sdk::derivatives_trading_usds_futures::websocket_streams;

pub fn from_response(
    res: &websocket_streams::IndividualSymbolBookTickerStreamsResponse,
) -> Option<BookTicker> {
    let ask_price = res.a.as_ref()?.parse::<f64>().ok()?;
    let bid_price = res.b.as_ref()?.parse::<f64>().ok()?;

    let ask_qty = res.a_uppercase.as_ref()?.parse::<f64>().ok()?;
    let bid_qty = res.b_uppercase.as_ref()?.parse::<f64>().ok()?;

    let transaction_time = res.t_uppercase?;

    Some(BookTicker::new(
        transaction_time,
        bid_price,
        bid_qty,
        ask_price,
        ask_qty,
    ))
}