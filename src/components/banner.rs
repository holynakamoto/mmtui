use crate::components::banner_frames::{
    BannerColor, BannerTheme, ball_row, basketball_frame, resolve, round_label, title_rows,
};
use ncaa_api::RoundKind;
use tui::buffer::Buffer;
use tui::layout::Rect;
use tui::style::Style;
use tui::text::{Line, Span};
use tui::widgets::{Block, BorderType, Borders, Widget};

pub use crate::components::banner_frames::FRAME_COUNT;

pub struct AnimatedBanner {
    pub frame: usize,
    pub tick: u64,
    pub theme: BannerTheme,
    pub view_round: RoundKind,
    pub current_round: RoundKind,
}

impl Default for AnimatedBanner {
    fn default() -> Self {
        Self {
            frame: 0,
            tick: 0,
            theme: BannerTheme::Dark,
            view_round: RoundKind::First,
            current_round: RoundKind::First,
        }
    }
}

impl Widget for AnimatedBanner {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width < 20 || area.height < 3 {
            render_line(
                Line::from(" MARCH MADNESS "),
                area.x,
                area.y,
                area.width,
                buf,
            );
            return;
        }

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Double)
            .border_style(resolve(BannerColor::Primary, self.theme));
        let inner = block.inner(area);
        block.render(area, buf);
        if inner.width == 0 || inner.height == 0 {
            return;
        }

        if inner.width < 80 {
            render_compact(&self, inner, buf);
            return;
        }
        render_full(&self, inner, buf);
    }
}

fn render_compact(banner: &AnimatedBanner, inner: Rect, buf: &mut Buffer) {
    let title = "MARCH MADNESS 2026";
    let round = format!(
        "{}  |  LIVE: {}",
        round_label(banner.view_round),
        round_label(banner.current_round)
    );
    render_centered(
        Line::from(Span::styled(title, resolve(BannerColor::Accent, banner.theme))),
        inner,
        inner.y,
        buf,
    );
    if inner.height > 1 {
        render_centered(
            Line::from(Span::styled(round, resolve(BannerColor::Secondary, banner.theme))),
            inner,
            inner.y + 1,
            buf,
        );
    }
}

fn render_full(banner: &AnimatedBanner, inner: Rect, buf: &mut Buffer) {
    let title = title_rows();
    let left_ball = basketball_frame(banner.frame);
    let right_ball = basketball_frame((banner.frame + 2) % FRAME_COUNT);
    let ball_y = ball_row(banner.tick, 5);
    let show_right_ball = inner.width > 100;

    for row in 0..4u16 {
        if row >= inner.height {
            break;
        }
        let y = inner.y + row;
        let ball_style = if row == ball_y {
            resolve(BannerColor::Secondary, banner.theme)
        } else {
            resolve(BannerColor::Shadow, banner.theme)
        };

        let mut spans = Vec::new();
        spans.push(Span::styled(left_ball[row as usize].to_string(), ball_style));
        spans.push(Span::raw(" "));
        spans.push(Span::styled(
            title[row as usize].to_string(),
            resolve(BannerColor::Primary, banner.theme),
        ));
        if show_right_ball {
            spans.push(Span::raw(" "));
            spans.push(Span::styled(right_ball[row as usize].to_string(), ball_style));
        }
        render_centered(Line::from(spans), inner, y, buf);
    }

    if inner.height > 4 {
        let round = format!(
            " {}  ->  {} ",
            round_label(banner.view_round),
            round_label(banner.current_round)
        );
        render_centered(
            Line::from(Span::styled(round, resolve(BannerColor::Accent, banner.theme))),
            inner,
            inner.y + 4,
            buf,
        );
    }
}

fn render_centered(line: Line, area: Rect, y: u16, buf: &mut Buffer) {
    if y >= area.y + area.height {
        return;
    }
    let w = line.width() as u16;
    let x = area.x + area.width.saturating_sub(w) / 2;
    render_line(line, x, y, area.width, buf);
}

fn render_line(line: Line, x: u16, y: u16, max_width: u16, buf: &mut Buffer) {
    let mut cx = x;
    let limit = x.saturating_add(max_width);
    for span in &line.spans {
        let text = span.content.as_ref();
        let style: Style = span.style;
        let mut run = String::new();
        for ch in text.chars() {
            if cx >= limit {
                break;
            }
            run.push(ch);
            cx += 1;
        }
        let start = cx.saturating_sub(run.chars().count() as u16);
        if !run.is_empty() {
            buf.set_string(start, y, run, style);
        }
    }
}
