use ratatui::{
    Frame, layout::{Constraint, Rect}, style::{Color, Style, Stylize}, text::{Line, Span}, widgets::{Block, Cell, Paragraph, Row, Table},
};

use crate::{domain::order_book::{OrderBook, PriceLevel}, ui::theme};

const PRICE_W: u16 = 12;
const QTY_W: u16 = 11;
const TOTAL_W: u16 = 11;
const COL_SPACING: u16 = 1;

// const ASK_FG: Color = Color::Red;
// const BID_FG: Color = Color::Green;
// const ASK_BAR: Color = Color::Rgb(130, 45, 45);
// const BID_BAR: Color = Color::Rgb(35, 110, 65);

type DepthLevel = (f64, f64, f64);

pub fn render_order_book(frame: &mut Frame, area: Rect, book: Option<&OrderBook>) {
    let Some(book) = book.filter(|b| !b.bid.is_empty() && !b.ask.is_empty()) else {
        let block = Block::bordered().title("  Order Book  ");
        let inner = block.inner(area);
        frame.render_widget(block, area);
        frame.render_widget(
            Paragraph::new("Waiting for depth data...")
                .dark_gray()
                .centered(),
            inner,
        );
        return;
    };

    let bid_total: f64 = book.bid.iter().map(|l| l.qty).sum();
    let ask_total: f64 = book.ask.iter().map(|l| l.qty).sum();
    let imbalance = bid_total / (bid_total + ask_total).max(f64::EPSILON);

    let block = theme::panel("Order Book").title_bottom(
        Line::from(vec![
            Span::styled(format!(" Bid {:.1}% ", imbalance * 100.0), Style::new().fg(theme::BID).bold()),
            Span::styled("│", Style::new().fg(theme::DIM)),
            Span::styled(format!(" Ask {:.1}% ", (1.0 - imbalance) * 100.0), Style::new().fg(theme::ASK).bold()),
        ])
        .centered(),
    );
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let per_side = (inner.height.saturating_sub(2) / 2) as usize;
    if per_side == 0 {
        return;
    }

    let asks = cumulative(&book.ask[..book.ask.len().min(per_side)]);
    let bids = cumulative(&book.bid[..book.bid.len().min(per_side)]);

    let max_cum = asks
        .last()
        .map(|l| l.2)
        .into_iter()
        .chain(bids.last().map(|l| l.2))
        .fold(0.0_f64, f64::max);

    let fixed = PRICE_W + QTY_W + TOTAL_W + COL_SPACING * 3;
    let bar_w = inner.width.saturating_sub(fixed) as usize;

    let mut rows: Vec<Row> = Vec::with_capacity(per_side * 2 + 1);

    for _ in asks.len()..per_side {
        rows.push(Row::new(vec![""]));
    }

    for &(price, qty, cum) in asks.iter().rev() {
        rows.push(level_row(price, qty, cum, max_cum, bar_w, theme::ASK, theme::ASK_BAR));
    }

    rows.push(spread_row(&book.bid[0], &book.ask[0]));

    for &(price, qty, cum) in &bids {
        rows.push(level_row(price, qty, cum, max_cum, bar_w, theme::BID, theme::BID_BAR));
    }

    let widths = [
        Constraint::Length(PRICE_W),
        Constraint::Length(QTY_W),
        Constraint::Length(TOTAL_W),
        Constraint::Fill(1),
    ];

    let header = Row::new(vec![
        format!("{:>w$}", "Price", w = PRICE_W as usize),
        format!("{:>w$}", "Size", w = QTY_W as usize),
        format!("{:>w$}", "Total", w = TOTAL_W as usize),
        "  Depth".to_string(),
    ])
    .style(Style::new().dark_gray().bold());

    frame.render_widget(
        Table::new(rows, widths)
            .header(header)
            .column_spacing(COL_SPACING),
        inner,
    );
}

fn cumulative(levels: &[PriceLevel]) -> Vec<DepthLevel> {
    levels
        .iter()
        .scan(0.0_f64, |acc, l| {
            *acc += l.qty;
            Some((l.price, l.qty, *acc))
        })
        .collect()
}

fn level_row(
    price: f64,
    qty: f64,
    cum: f64,
    max_cum: f64,
    bar_w: usize,
    fg: Color,
    bar_color: Color,
) -> Row<'static> {
    Row::new(vec![
        Cell::from(format!("{price:>w$.2}", w = PRICE_W as usize))
            .style(Style::new().fg(fg).bold()),
        Cell::from(format!("{qty:>w$.4}", w = QTY_W as usize)),
        Cell::from(format!("{cum:>w$.4}", w = TOTAL_W as usize)).dark_gray(),
        Cell::from(depth_bar(cum, max_cum, bar_w)).style(Style::new().fg(bar_color)),
    ])
}

fn spread_row(best_bid: &PriceLevel, best_ask: &PriceLevel) -> Row<'static> {
    let spread = best_ask.price - best_bid.price;
    let mid = (best_ask.price + best_bid.price) / 2.0;
    let bps = if mid > 0.0 {
        spread / mid * 10_000.0
    } else {
        0.0
    };

    Row::new(vec![
        Cell::from(format!("{mid:>w$.2}", w = PRICE_W as usize)).bold(),
        Cell::from(format!("{:>w$}", "spread", w = QTY_W as usize)),
        Cell::from(format!("{spread:>w$.2}", w = TOTAL_W as usize)),
        Cell::from(format!("  {bps:.2} bps")),
    ])
    .style(Style::new().fg(Color::Yellow))
}

fn depth_bar(value: f64, max: f64, width: usize) -> String {
    const PARTIAL: [char; 8] = [' ', '▏', '▎', '▍', '▌', '▋', '▊', '▉'];

    if max <= 0.0 || width == 0 {
        return String::new();
    }

    let eighths = ((value / max) * (width * 8) as f64).round() as usize;
    let eighths = eighths.min(width * 8);
    let (full, rem) = (eighths / 8, eighths % 8);

    let mut bar = "█".repeat(full);
    if rem > 0 {
        bar.push(PARTIAL[rem]);
    }
    bar
}
