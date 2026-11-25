mod app;
mod domain;
mod infrastructure;
mod ui;
mod util;

use std::sync::Arc;

use app::Channels;
use domain::agg_trades::AggTrades;
use domain::book_ticker::BookTicker;
use domain::engine::Engine;
use domain::executor::{BinanceGateway, ExecConfig, Fill, OrderCommand};
use domain::order_book::OrderBook;
use domain::signal::SignalConfig;
use infrastructure::binance::order_executor::BinanceOrderExecutor;
use infrastructure::binance::{agg_trades_mapper, book_depth_mapper, book_ticker_mapper};

use binance_sdk::config::{ConfigurationWebsocketApi, ConfigurationWebsocketStreams};
use binance_sdk::derivatives_trading_usds_futures::{
    websocket_streams, DerivativesTradingUsdsFuturesWsApi, DerivativesTradingUsdsFuturesWsStreams,
};
use tokio::sync::mpsc;

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    let api_key = std::env::var("BINANCE_API_KEY").unwrap_or_default();
    let api_secret = std::env::var("BINANCE_API_SECRET").unwrap_or_default();

    let api_config = ConfigurationWebsocketStreams::builder()
        .api_key(&api_key)
        .api_secret(&api_secret)
        .build()?;

    let ws_api_config = ConfigurationWebsocketApi::builder()
        .api_key(&api_key)
        .api_secret(&api_secret)
        .build()?;

    let client = DerivativesTradingUsdsFuturesWsStreams::testnet(api_config);
    let ws_api_client = DerivativesTradingUsdsFuturesWsApi::testnet(ws_api_config);

    let connection = client.connect().await?;
    let ws_connection = ws_api_client.connect().await?;

    let (bt_tx, bt_rx) = mpsc::unbounded_channel::<BookTicker>();
    let (at_tx, at_rx) = mpsc::unbounded_channel::<AggTrades>();
    let (pd_tx, pd_rx) = mpsc::unbounded_channel::<OrderBook>();
    let (fill_tx, fill_rx) = mpsc::unbounded_channel::<Fill>();
    let (order_tx, order_rx) = mpsc::unbounded_channel::<OrderCommand>();

    let symbol = "BTCUSDT".to_string();

    let book_ticker_params =
        websocket_streams::IndividualSymbolBookTickerStreamsParams::builder(symbol.clone())
            .build()?;
    let agg_trades_params =
        websocket_streams::AggregateTradeStreamsParams::builder(symbol.clone()).build()?;
    let partial_depth_params = websocket_streams::PartialBookDepthStreamsParams::builder(
        symbol.clone(),
        websocket_streams::PartialBookDepthStreamsLevelsEnum::Levels20,
    )
    .update_speed(websocket_streams::PartialBookDepthStreamsUpdateSpeedEnum::UpdateSpeed100ms)
    .build()?;

    let book_ticker_stream = connection
        .individual_symbol_book_ticker_streams(book_ticker_params)
        .await?;
    let agg_trades_stream = connection
        .aggregate_trade_streams(agg_trades_params)
        .await?;
    let partial_depth_stream = connection
        .partial_book_depth_streams(partial_depth_params)
        .await?;

    book_ticker_stream.on_message(move |data| {
        if let Some(bt) = book_ticker_mapper::from_response(&data) {
            let _ = bt_tx.send(bt);
        }
    });
    agg_trades_stream.on_message(move |data| {
        if let Some(at) = agg_trades_mapper::from_response(&data) {
            let _ = at_tx.send(at);
        }
    });
    partial_depth_stream.on_message(move |data| {
        if let Some(book) = book_depth_mapper::from_response(&data) {
            let _ = pd_tx.send(book);
        }
    });

    let live_mode = std::env::var("BINANCE_LIVE")
        .map(|v| v == "1" || v == "true")
        .unwrap_or(false);

    let engine = if live_mode {
        let executor = Arc::new(BinanceOrderExecutor::new(symbol.clone(), ws_connection.clone()));
        if !api_key.is_empty() {
            let _ = executor
                .start_user_data_listener(&connection, fill_tx.clone())
                .await;
        }
        executor.spawn_worker(order_rx, fill_tx);

        let binance_gw = BinanceGateway::new(order_tx);
        Engine::new_live(SignalConfig::default(), ExecConfig::default(), binance_gw)
    } else {
        Engine::new(SignalConfig::default(), ExecConfig::default(), 40)
    };

    let mut channels = Channels {
        bt_rx,
        at_rx,
        pd_rx,
        fill_rx,
    };
    ratatui::run(|terminal| app::run(terminal, &mut channels, engine))?;

    let _ = ws_connection.disconnect().await;
    connection.disconnect().await?;
    println!("Disconnect");
    Ok(())
}
