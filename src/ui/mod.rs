mod info;
mod order_book;
mod pnl;
mod tables;
mod theme;
use ratatui::style::Style;
use ratatui::widgets::Block;

use ratatui::{
    Frame,
    layout::{Constraint, Layout},
};

use crate::app::AppState;
use crate::domain::signal::Snapshot;

const FOOTER_HEIGHT: u16 = 12;

pub fn render(frame: &mut Frame, state: &AppState, snap: Option<&Snapshot>) {

    frame.render_widget(
        Block::default().style(Style::new().bg(theme::BG).fg(theme::TEXT)),
        frame.area(),
    );

    let [top, footer] =
        Layout::vertical([Constraint::Fill(1), Constraint::Length(FOOTER_HEIGHT)])
            .areas(frame.area());

    let [left, right] =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(top);

    pnl::render_footer_pnl(frame, footer, &state.pnl);
    order_book::render_order_book(frame, left, state.order_book.as_ref());

    let [r_top, r_mid, r_bottom] = Layout::vertical([Constraint::Fill(1); 3]).areas(right);

    let ticker_block = theme::panel("Book Ticker");
    let ticker_inner = ticker_block.inner(r_top);
    frame.render_widget(ticker_block, r_top);
    tables::render_book_ticker_table(
        frame,
        ticker_inner,
        state.latest_bt.as_ref(),
        &state.history_bt,
    );

    let trades_block = theme::panel("Agg Trades");
    let trades_inner = trades_block.inner(r_mid);
    frame.render_widget(trades_block, r_mid);
    tables::render_agg_trades_table(
        frame,
        trades_inner,
        state.latest_at.as_ref(),
        &state.history_at,
    );

    let info_block = theme::panel(&format!(
        "Signals · {} · {}",
        if state.armed { "ARMED" } else { "OFF (t)" },
        state.exec_label
    ));
    let info_inner = info_block.inner(r_bottom);
    frame.render_widget(info_block, r_bottom);
    info::render_info(frame, info_inner, snap);
}