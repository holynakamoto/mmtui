use tui::layout::{Constraint, Layout, Rect, Size};
pub const TAB_BAR_HEIGHT: u16 = 3;

/// Pre-computed layout areas for the main draw loop.
pub struct LayoutAreas {
    pub tab_bar: [Rect; 2],
    pub main: Rect,
}

impl LayoutAreas {
    pub fn new(size: Size) -> Self {
        let rect = Rect::new(0, 0, size.width, size.height);
        Self::from_rect(rect, false)
    }

    pub fn update(&mut self, area: Rect, full_screen: bool) {
        *self = Self::from_rect(area, full_screen);
    }

    fn from_rect(area: Rect, full_screen: bool) -> Self {
        if full_screen {
            let [main] = Layout::vertical([Constraint::Fill(1)]).areas(area);
            return LayoutAreas {
                tab_bar: [Rect::ZERO, Rect::ZERO],
                main,
            };
        }

        let [tab, main] = Layout::vertical([
            Constraint::Length(TAB_BAR_HEIGHT),
            Constraint::Fill(1),
        ])
        .areas(area);

        LayoutAreas {
            tab_bar: Self::split_tab_bar(tab),
            main,
        }
    }

    fn split_tab_bar(area: Rect) -> [Rect; 2] {
        Layout::horizontal([Constraint::Percentage(85), Constraint::Percentage(15)]).areas(area)
    }
}
