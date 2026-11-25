use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;
use rust_decimal::Decimal;

use binance_sdk::derivatives_trading_usds_futures::{
    websocket_api::{
        CancelOrderParams, NewOrderParams, NewOrderReduceOnlyEnum, NewOrderSideEnum,
        NewOrderTimeInForceEnum, NewOrderTypeEnum, StartUserDataStreamParams, WebsocketApi,
    },
    websocket_streams::{UserDataStreamEventsResponse, WebsocketStreams},
};

use crate::domain::executor::{Fill, OrderCommand, Side};
use crate::util::now_ms;

pub struct BinanceOrderExecutor {
    symbol: String,
    ws_api: WebsocketApi,
    active_order_id: Arc<AtomicI64>,
}

impl BinanceOrderExecutor {
    pub fn new(symbol: String, ws_api: WebsocketApi) -> Self {
        Self {
            symbol,
            ws_api,
            active_order_id: Arc::new(AtomicI64::new(0)),
        }
    }

    /// Memulai listener User Data Stream untuk menangkap fill secara real-time dari Binance.
    pub async fn start_user_data_listener(
        &self,
        streams_connection: &WebsocketStreams,
        fill_tx: mpsc::UnboundedSender<Fill>,
    ) -> Result<(), anyhow::Error> {
        let params = StartUserDataStreamParams::default();
        let resp = self.ws_api.start_user_data_stream(params).await?;
        let data = resp.data()?;
        
        let Some(listen_key) = data.listen_key else {
            return Err(anyhow::anyhow!("Listen key tidak ditemukan dari Binance"));
        };

        let stream = streams_connection.user_data(listen_key, None).await?;
        let symbol_filter = self.symbol.clone();

        stream.on_message(move |event| {
            if let UserDataStreamEventsResponse::OrderTradeUpdate(update) = event {
                if let Some(o) = update.o {
                    if let Some(ref sym) = o.s {
                        if sym != &symbol_filter {
                            return;
                        }
                    }

                    let status = o.x_uppercase.as_deref().unwrap_or_default();
                    if status == "FILLED" || status == "PARTIALLY_FILLED" {
                        let price = o
                            .ap
                            .as_deref()
                            .and_then(|p| p.parse::<f64>().ok())
                            .or_else(|| o.l_uppercase.as_deref().and_then(|p| p.parse::<f64>().ok()))
                            .unwrap_or(0.0);
                        let maker = o.m.unwrap_or(false);
                        let ts = o.t_uppercase.unwrap_or_else(now_ms);

                        let is_reduce_only = o.r_uppercase.unwrap_or(false);
                        let fill = if is_reduce_only {
                            Fill::Exit { price, maker, ts }
                        } else {
                            let side = if o.s_uppercase.as_deref() == Some("BUY") {
                                Side::Buy
                            } else {
                                Side::Sell
                            };
                            Fill::Entry {
                                side,
                                price,
                                maker,
                                ts,
                            }
                        };
                        let _ = fill_tx.send(fill);
                    }
                }
            }
        });

        Ok(())
    }

    /// Task background untuk memproses perintah order dari engine.
    pub fn spawn_worker(
        self: Arc<Self>,
        mut rx: mpsc::UnboundedReceiver<OrderCommand>,
        fill_tx: mpsc::UnboundedSender<Fill>,
    ) {
        tokio::spawn(async move {
            while let Some(cmd) = rx.recv().await {
                let this = self.clone();
                let f_tx = fill_tx.clone();

                match cmd {
                    OrderCommand::PostOnly {
                        side,
                        price,
                        qty,
                        reduce_only,
                        now,
                    } => {
                        let binance_side = match side {
                            Side::Buy => NewOrderSideEnum::Buy,
                            Side::Sell => NewOrderSideEnum::Sell,
                        };

                        let mut builder = NewOrderParams::builder(
                            this.symbol.clone(),
                            binance_side,
                            NewOrderTypeEnum::Limit,
                        );

                        // Format desimal dengan presisi yang aman
                        if let Some(p) = Decimal::from_f64_retain(price) {
                            builder = builder.price(p.round_dp(2));
                        }
                        if let Some(q) = Decimal::from_f64_retain(qty) {
                            builder = builder.quantity(q.round_dp(3));
                        }

                        builder = builder.time_in_force(NewOrderTimeInForceEnum::Gtx);

                        if reduce_only {
                            builder = builder.reduce_only(NewOrderReduceOnlyEnum::True);
                        }

                        if let Ok(params) = builder.build() {
                            match this.ws_api.new_order(params).await {
                                Ok(resp) => {
                                    if let Ok(data) = resp.data() {
                                        if let Some(order_id) = data.order_id {
                                            this.active_order_id.store(order_id, Ordering::SeqCst);
                                        }
                                        // Jika order langsung terisi (market/cross):
                                        if data.status.as_deref() == Some("FILLED") {
                                            let fill_px = data
                                                .price
                                                .as_deref()
                                                .and_then(|p| p.parse::<f64>().ok())
                                                .unwrap_or(price);
                                            let fill = if reduce_only {
                                                Fill::Exit {
                                                    price: fill_px,
                                                    maker: true,
                                                    ts: now,
                                                }
                                            } else {
                                                Fill::Entry {
                                                    side,
                                                    price: fill_px,
                                                    maker: true,
                                                    ts: now,
                                                }
                                            };
                                            let _ = f_tx.send(fill);
                                        }
                                    }
                                }
                                Err(e) => {
                                    eprintln!("[Binance WS API] Gagal kirim PostOnly: {:?}", e);
                                }
                            }
                        }
                    }

                    OrderCommand::Market {
                        side,
                        qty,
                        reduce_only,
                        now,
                    } => {
                        let binance_side = match side {
                            Side::Buy => NewOrderSideEnum::Buy,
                            Side::Sell => NewOrderSideEnum::Sell,
                        };

                        let mut builder = NewOrderParams::builder(
                            this.symbol.clone(),
                            binance_side,
                            NewOrderTypeEnum::Market,
                        );

                        if let Some(q) = Decimal::from_f64_retain(qty) {
                            builder = builder.quantity(q.round_dp(3));
                        }

                        if reduce_only {
                            builder = builder.reduce_only(NewOrderReduceOnlyEnum::True);
                        }

                        if let Ok(params) = builder.build() {
                            match this.ws_api.new_order(params).await {
                                Ok(resp) => {
                                    if let Ok(data) = resp.data() {
                                        let fill_px = data
                                            .price
                                            .as_deref()
                                            .and_then(|p| p.parse::<f64>().ok())
                                            .unwrap_or(0.0);
                                        let fill = if reduce_only {
                                            Fill::Exit {
                                                price: fill_px,
                                                maker: false,
                                                ts: now,
                                            }
                                        } else {
                                            Fill::Entry {
                                                side,
                                                price: fill_px,
                                                maker: false,
                                                ts: now,
                                            }
                                        };
                                        let _ = f_tx.send(fill);
                                    }
                                }
                                Err(e) => {
                                    eprintln!("[Binance WS API] Gagal kirim Market order: {:?}", e);
                                }
                            }
                        }
                    }

                    OrderCommand::CancelAll { .. } => {
                        let order_id = this.active_order_id.swap(0, Ordering::SeqCst);
                        if order_id > 0 {
                            let builder = CancelOrderParams::builder(this.symbol.clone())
                                .order_id(order_id);
                            if let Ok(params) = builder.build() {
                                let _ = this.ws_api.cancel_order(params).await;
                            }
                        }
                    }
                }
            }
        });
    }
}
