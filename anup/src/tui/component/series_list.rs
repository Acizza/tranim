use super::{Component, Draw};
use crate::err::Result;
use crate::series::LastWatched;
use crate::tui::UIState;
use crate::CmdOptions;
use termion::event::Key;
use tui::backend::Backend;
use tui::layout::Rect;
use tui::style::{Color, Modifier, Style};
use tui::terminal::Frame;
use tui::widgets::{Block, Borders, List, ListState, Text};

pub struct SeriesList {
    list_state: ListState,
}

impl SeriesList {
    pub fn init(args: &CmdOptions, state: &mut UIState, last_watched: &LastWatched) -> Self {
        let selected = {
            let desired_series = args.series.as_ref().or_else(|| last_watched.get());

            match desired_series {
                Some(desired) => state
                    .series
                    .iter()
                    .position(|series| series.nickname() == desired)
                    .unwrap_or(0),
                None => 0,
            }
        };

        state.series.set_selected(selected);
        state.init_selected_series().ok();

        Self {
            list_state: ListState::default(),
        }
    }
}

impl Component for SeriesList {
    type TickResult = ();
    type KeyResult = ();

    fn process_key(&mut self, key: Key, state: &mut UIState) -> Result<Self::KeyResult> {
        match key {
            Key::Up | Key::Down => {
                match key {
                    Key::Up => state.series.dec_selected(),
                    Key::Down => state.series.inc_selected(),
                    _ => unreachable!(),
                }

                state.init_selected_series()
            }
            _ => Ok(()),
        }
    }
}

impl<B> Draw<B> for SeriesList
where
    B: Backend,
{
    fn draw(&mut self, state: &UIState, rect: Rect, frame: &mut Frame<B>) {
        let series_names = state
            .series
            .iter()
            .map(|series| Text::raw(series.nickname()));

        let series_list = List::new(series_names)
            .block(Block::default().title("Series").borders(Borders::ALL))
            .style(Style::default().fg(Color::White))
            .highlight_style(Style::default().fg(Color::Green).modifier(Modifier::ITALIC))
            .highlight_symbol(">");

        self.list_state.select(Some(state.series.index()));

        frame.render_stateful_widget(series_list, rect, &mut self.list_state);
    }
}
