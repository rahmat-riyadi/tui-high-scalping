use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Style, Stylize},
    text::{Line, Span},
    widgets::Paragraph,
};

use crate::{
    domain::signal::{Direction, ExecMode, Snapshot},
    ui::theme,
};

const LABEL_W: usize = 10;
const VALUE_W: usize = 8;

/// Warna berdasarkan tanda sinyal; dead zone kecil dianggap netral.
fn signal_color(v: f64) -> Color {
    if v > 0.05 { theme::BID } else if v < -0.05 { theme::ASK } else { theme::DIM }
}


fn bipolar_bar(v: f64, width: usize) -> Vec<Span<'static>> {
    let half = width.saturating_sub(1) / 2;
    let v = v.clamp(-1.0, 1.0);
    let fill = (v.abs() * half as f64).round() as usize;
    let (neg, pos) = if v < 0.0 { (fill, 0) } else { (0, fill) };

    vec![
        Span::raw(" ".repeat(half - neg)),
        Span::styled("█".repeat(neg), Style::new().fg(theme::ASK)),
        Span::styled("│", theme::DIM),
        Span::styled("█".repeat(pos), Style::new().fg(theme::BID)),
        Span::raw(" ".repeat(half - pos)),
    ]
}

fn signal_row(label: &str, v: f64, shown: String, bar_w: usize) -> Line<'static> {
    let mut spans = vec![Span::styled(
        format!("{label:<w$}", w = LABEL_W),
        theme::DIM,
    )];
    spans.extend(bipolar_bar(v, bar_w));
    spans.push(Span::styled(
        format!(" {shown:>w$}", w = VALUE_W - 1),
        Style::new().fg(signal_color(v)).bold(),
    ));
    Line::from(spans)
}

fn kv(label: &str, value: String, color: Color) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{label:<w$}", w = LABEL_W), theme::DIM),
        Span::styled(value, Style::new().fg(color).bold()),
    ])
}

fn gate_span(label: &str, ok: bool) -> Span<'static> {
    let (mark, color) = if ok {
        ("✔", theme::BID)
    } else {
        ("✘", theme::ASK)
    };
    Span::styled(format!("{mark} {label}  "), Style::new().fg(color))
}

pub fn render_info(frame: &mut Frame, area: Rect, snap: Option<&Snapshot>) {
    let Some(s) = snap else {
        frame.render_widget(
            Paragraph::new("Waiting for signals...")
                .dark_gray()
                .centered(),
            area,
        );
        return;
    };

    let [l, r] =
        Layout::horizontal([Constraint::Percentage(58), Constraint::Fill(1)]).areas(area);

    render_signals(frame, l, s);
    render_status(frame, r, s);
}

fn render_signals(frame: &mut Frame, area: Rect, s: &Snapshot) {
    let bar_w = (area.width as usize).saturating_sub(LABEL_W + VALUE_W);

    let (dir_text, dir_color) = match s.direction {
        Direction::Long => ("LONG", theme::BID),
        Direction::Short => ("SHORT", theme::ASK),
        Direction::Flat => ("FLAT", theme::DIM),
    };

    let lines = vec![
        Line::from(vec![
            Span::styled(
                format!(" {dir_text} "),
                Style::new().fg(theme::BG).bg(dir_color).bold(),
            ),
            Span::styled(format!("  score {:+.2}", s.score), Style::new().bold()),
        ]),
        signal_row(
            "Micro-mid",
            s.micro_norm,
            format!("{:+.2}t", s.micro_ticks),
            bar_w,
        ),
        signal_row("Imb L1", s.imb_l1, format!("{:+.2}", s.imb_l1), bar_w),
        signal_row("Imb L5", s.imb_l5, format!("{:+.2}", s.imb_l5), bar_w),
        signal_row("Imb L20", s.imb_l20, format!("{:+.2}", s.imb_l20), bar_w),
        signal_row("OFI 100ms", s.ofi_100ms, format!("{:+.2}", s.ofi_100ms), bar_w),
        signal_row("OFI 1s", s.ofi_1s, format!("{:+.2}", s.ofi_1s), bar_w),
        signal_row("Δ Trades", s.flow_1s, format!("{:+.2}", s.flow_1s), bar_w),
    ];

    frame.render_widget(Paragraph::new(lines), area);
}

fn render_status(frame: &mut Frame, area: Rect, s: &Snapshot) {
    let (mode_text, mode_color) = match s.exec_mode {
        ExecMode::Maker => ("MAKER", theme::BID),
        ExecMode::Taker => ("TAKER", theme::WARN),
        ExecMode::Skip => ("SKIP", theme::DIM),
    };
    let edge_color = if s.net_edge_bps > 0.0 {
        theme::BID
    } else {
        theme::ASK
    };
    let spread_color = if s.gates.spread {
        theme::TEXT
    } else {
        theme::WARN
    };
    let lat_color = if s.gates.latency {
        theme::TEXT
    } else {
        theme::ASK
    };

    // 8 baris, sama tinggi dengan panel sinyal di kiri.
    let lines = vec![
        kv("Mode", mode_text.to_string(), mode_color),
        kv(
            "Edge M|T",
            format!("{:+.2} | {:+.2} bps", s.net_edge_maker_bps, s.net_edge_taker_bps),
            edge_color,
        ),
        kv(
            "Cost M|T",
            format!("{:.2} | {:.2} bps", s.cost_maker_bps, s.cost_taker_bps),
            theme::DIM,
        ),
        kv(
            "Spread",
            format!("{:.0}t / {:.3}bps", s.spread_ticks, s.spread_bps),
            spread_color,
        ),
        kv("Vol 10s", format!("{:.2} bps", s.vol_10s_bps), theme::TEXT),
        kv(
            "Trades/s",
            format!("{:.0} ({:.1}x)", s.trades_per_s, s.intensity),
            theme::TEXT,
        ),
        kv(
            "Latency",
            format!("{} ms | age {} ms", s.latency_ms, s.feed_age_ms),
            lat_color,
        ),
        Line::from(vec![
            gate_span("SIG", s.gates.signal),
            gate_span("SPR", s.gates.spread),
            gate_span("EDGE", s.gates.edge),
            gate_span("LAT", s.gates.latency),
        ]),
    ];

    frame.render_widget(Paragraph::new(lines), area);
}