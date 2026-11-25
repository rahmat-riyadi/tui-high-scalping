use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders},
};

pub const BG: Color     = Color::Rgb(22, 24, 33);    // latar
pub const TEXT: Color   = Color::Rgb(192, 202, 245); // teks utama
pub const MUTED: Color  = Color::Rgb(130, 140, 180); // data lama / history
pub const DIM: Color    = Color::Rgb(86, 95, 137);   // label
pub const BORDER: Color = Color::Rgb(65, 72, 104);   // garis panel
pub const TITLE: Color  = Color::Rgb(122, 162, 247); // judul panel
pub const BID: Color    = Color::Rgb(115, 218, 202); // hijau-teal
pub const ASK: Color    = Color::Rgb(247, 118, 142); // merah lembut
pub const WARN: Color   = Color::Rgb(224, 175, 104); // amber
pub const BID_BAR: Color = Color::Rgb(38, 94, 88);   // bar depth bid
pub const ASK_BAR: Color = Color::Rgb(112, 52, 68);  // bar depth ask

pub fn panel(title: &str) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(BORDER))
        .title(Line::from(Span::styled(
            format!(" {title} "),
            Style::new().fg(TITLE).add_modifier(Modifier::BOLD),
        )))
}