use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Style, Stylize},
    symbols::Marker,
    text::{Line, Span},
    widgets::{Axis, Block, Chart, Dataset, GraphType, Paragraph},
};

use crate::{domain::pnl::PnlStats, ui::theme};

// Pindahkan: render_footer_pnl (pub), pnl_color, render_pnl_chart, render_pnl_stats, stat_card
pub fn render_footer_pnl(frame: &mut Frame, area: Rect, stats: &PnlStats) {
    let [l, r] =
        Layout::horizontal([Constraint::Percentage(60), Constraint::Percentage(40)]).areas(area);

    render_pnl_chart(frame, l, stats);
    render_pnl_stats(frame, r, stats);
}

fn pnl_color(v: f64) -> Color {
    if v > 0.0 {
        theme::BID
    } else if v < 0.0 {
        theme::ASK
    } else {
        theme::DIM
    }
}

fn render_pnl_chart(frame: &mut Frame, area: Rect, stats: &PnlStats) {
    let block = Block::bordered().title(" PnL Curve ");

    if stats.equity.len() < 2 {
        let inner = block.inner(area);
        frame.render_widget(block, area);
        frame.render_widget(
            Paragraph::new("No trades yet").dark_gray().centered(),
            inner,
        );
        return;
    }

    let x_min = stats.equity.first().map(|p| p.0).unwrap_or(0.0);
    let x_max = stats.equity.last().map(|p| p.0).unwrap_or(1.0);

    let (lo, hi) = stats
        .equity
        .iter()
        .fold((f64::MAX, f64::MIN), |(lo, hi), &(_, y)| {
            (lo.min(y), hi.max(y))
        });

    let (lo, hi) = (lo.min(0.0), hi.max(0.0));
    let pad = ((hi - lo) * 0.1).max(0.01);
    let (y_min, y_max) = (lo - pad, hi + pad);

    let zero_line = [(x_min, 0.0), (x_max, 0.0)];

    let datasets = vec![
        Dataset::default()
            .marker(Marker::Braille)
            .graph_type(GraphType::Line)
            .style(theme::DIM)
            .data(&zero_line),
        Dataset::default()
            .marker(Marker::Braille)
            .graph_type(GraphType::Line)
            .style(Style::new().fg(pnl_color(stats.total_pnl)))
            .data(&stats.equity),
    ];

    let y_labels = vec![
        format!("{y_min:.2}"),
        format!("{:.2}", (y_min + y_max) / 2.0),
        format!("{y_max:.2}"),
    ];

    let chart = Chart::new(datasets)
        .block(block)
        .x_axis(Axis::default().bounds([x_min, x_max]))
        .y_axis(
            Axis::default()
                .style(theme::DIM)
                .bounds([y_min, y_max])
                .labels(y_labels),
        );

    frame.render_widget(chart, area);
}

fn render_pnl_stats(frame: &mut Frame, area: Rect, s: &PnlStats) {
    let [top, bottom] = Layout::vertical([Constraint::Fill(1), Constraint::Fill(1)]).areas(area);
    let [a, b] = Layout::horizontal([Constraint::Fill(1), Constraint::Fill(1)]).areas(top);
    let [c, d] = Layout::horizontal([Constraint::Fill(1), Constraint::Fill(1)]).areas(bottom);

    stat_card(
        frame,
        a,
        "PNL",
        format!("{:+.2}", s.total_pnl),
        Some(format!("{:+.2}%", s.pnl_pct)),
        pnl_color(s.total_pnl),
    );
    stat_card(
        frame,
        b,
        "TRADES",
        s.trades.to_string(),
        Some(format!("win {:.1}%", s.win_rate * 100.0)),
        theme::TITLE,
    );
    stat_card(
        frame,
        c,
        "MAX DD",
        format!("-{:.2}", s.max_drawdown.abs()),
        None,
        theme::ASK,
    );
    stat_card(
        frame,
        d,
        "CURRENT DD",
        format!("-{:.2}", s.current_drawdown.abs()),
        None,
        if s.current_drawdown.abs() > 0.0 {
            theme::WARN
        } else {
            theme::DIM
        },
    );
}

fn stat_card(
    frame: &mut Frame,
    area: Rect,
    label: &str,
    value: String,
    sub: Option<String>,
    color: Color,
) {
    let block = Block::bordered()
        .title(format!(" {label} "))
        .border_style(theme::TEXT);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines = vec![Line::from(Span::styled(
        value,
        Style::new().fg(color).bold(),
    ))];
    if let Some(sub) = sub {
        lines.push(Line::from(sub).white());
    }

    // Center secara vertikal.
    let top_pad = (inner.height as usize).saturating_sub(lines.len()) / 2;
    let mut all = vec![Line::raw(""); top_pad];
    all.extend(lines);

    frame.render_widget(Paragraph::new(all).centered(), inner);
}
