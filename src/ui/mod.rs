use crossterm::event::{self, EventStream, KeyCode};
use futures::stream::StreamExt;
use ratatui::{DefaultTerminal, Frame};
use tokio::time::{self, Duration};
use tui_logger::{TuiLoggerLevelOutput, TuiLoggerSmartWidget, TuiWidgetState};

pub struct Ui {
    logger_state: TuiWidgetState,
}

impl Ui {
    const TICK_INTERVAL: Duration = Duration::from_millis(40);

    pub async fn run(&self, terminal: &mut DefaultTerminal) -> anyhow::Result<()> {
        tui_logger::init_logger(tui_logger::LevelFilter::Off)?;
        tui_logger::set_default_level(tui_logger::LevelFilter::Info);
        let mut events = EventStream::new();

        loop {
            tokio::select! {
                _  = time::sleep(Self::TICK_INTERVAL) => {
                    terminal.draw(Self::render)?;
                }
                Some(event) = events.next() => {
                    if let event::Event::Key(key) = event? {
                        if let Some(res) = self.handle_key_event(key.code) {
                            break res;
                        }
                    }
                }
            }
        }
    }

    fn handle_key_event(&self, key_code: KeyCode) -> Option<anyhow::Result<()>> {
        type Twe = tui_logger::TuiWidgetEvent;
        type KC = KeyCode;
        match key_code {
            KC::Char('q') => return Some(Ok(())),
            KC::Up => self.logger_state.transition(Twe::UpKey),
            KC::Down => self.logger_state.transition(Twe::DownKey),
            KC::PageDown => self.logger_state.transition(Twe::NextPageKey),
            KC::PageUp => self.logger_state.transition(Twe::PrevPageKey),
            KC::Left => self.logger_state.transition(Twe::LeftKey),
            KC::Right => self.logger_state.transition(Twe::RightKey),
            _ => {}
        }

        None
    }

    fn render(frame: &mut Frame) {
        let logger_widget = TuiLoggerSmartWidget::default()
            .output_separator(':')
            .output_timestamp(Some("%H:%M:%S".to_string()))
            .output_level(Some(TuiLoggerLevelOutput::Abbreviated))
            .style_error(ratatui::style::Style::default().fg(ratatui::style::Color::Red))
            .style_warn(ratatui::style::Style::default().fg(ratatui::style::Color::Yellow))
            .output_target(true)
            .style_info(ratatui::style::Style::default().fg(ratatui::style::Color::Cyan));

        // 3. Render it to the full frame area
        frame.render_widget(logger_widget, frame.area());
    }
}

impl Default for Ui {
    fn default() -> Self {
        Ui {
            logger_state: TuiWidgetState::new(),
        }
    }
}
