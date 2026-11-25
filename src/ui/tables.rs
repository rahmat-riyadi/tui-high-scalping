use std::collections::VecDeque;

use ratatui::{
    Frame,
    layout::{Constraint, Rect},
    style::{Style},
    widgets::{Row, Table},
};

use crate::{domain::agg_trades::AggTrades, ui::theme};
use crate::domain::book_ticker::BookTicker;
use crate::util::fmt_time;

pub fn render_book_ticker_table(
    frame: &mut Frame,
    area: Rect,
    latest: Option<&BookTicker>,
    history: &VecDeque<BookTicker>,
) {
    let widths = [Constraint::Fill(1); 9];
    let visible = area.height.saturating_sub(1) as usize;

    let rows: Vec<Row> = latest
        .into_iter()
        .chain(history.iter())
        .take(visible)
        .enumerate()
        .map(|(i, bt)| {
            let row = Row::new(vec![
                fmt_time(bt.transaction_time),
                format!("{:.2}", bt.best_bid_price),
                format!("{:.3}", bt.best_bid_qty),
                format!("{:.2}", bt.best_ask_price),
                format!("{:.3}", bt.best_ask_qty),
                format!("{:.2}", bt.spread),
                format!("{:.2}", bt.mid_price),
                format!("{:.3}", bt.imbalance),
                format!("{:.2}", bt.micro),
            ]);
            if i == 0 {
                row.style(Style::new().bold())
            } else {
                row
            }
        })
        .collect();

    let header = Row::new(vec![
        "Time (UTC)",
        "Bid Price",
        "Bid Qty",
        "Ask Price",
        "Ask Qty",
        "Spread",
        "Mid Price",
        "Imb",
        "Micro",
    ])
    .style(Style::new().underlined());

    frame.render_widget(Table::new(rows, widths).header(header), area);
}

pub fn render_agg_trades_table(
    frame: &mut Frame,
    area: Rect,
    latest: Option<&AggTrades>,
    history: &VecDeque<AggTrades>,
) {
    let header =
        Row::new(vec!["Trade Time", "Price", "QTY", "MM"]).style(Style::new().underlined());
    let widths = [Constraint::Fill(1); 4];

    let visible = area.height.saturating_sub(1) as usize;

    let rows: Vec<Row> = latest
        .into_iter()
        .chain(history.iter())
        .take(visible)
        .enumerate()
        .map(|(i, at)| {
            let row = Row::new(vec![
                fmt_time(at.trade_time),
                format!("{:.2}", at.price),
                format!("{:.3}", at.qty),
                format!("{}", at.maker),
            ]);
            if i == 0 {
                row.style(Style::new().fg(theme::TEXT).bold())
            } else {
                row.style(Style::new().fg(theme::MUTED))
            }
        })
        .collect();

    frame.render_widget(Table::new(rows, widths).header(header), area);
}

