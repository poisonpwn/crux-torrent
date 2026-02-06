use crossterm::event;
use ratatui::{DefaultTerminal, Frame};
use tui_logger::{TuiLoggerLevelOutput, TuiLoggerSmartWidget, TuiWidgetState};

pub struct App {
    logger_state: TuiWidgetState,
}

impl App {
    pub fn run(&self, terminal: &mut DefaultTerminal) -> anyhow::Result<()> {
        tui_logger::init_logger(tui_logger::LevelFilter::Off)?;
        tui_logger::set_default_level(tui_logger::LevelFilter::Info);

        loop {
            terminal.draw(Self::render)?;

            if let event::Event::Key(key) = event::read()? {
                type KC = event::KeyCode;
                type Twe = tui_logger::TuiWidgetEvent;
                match key.code {
                    KC::Char('q') => break Ok(()),
                    KC::Up => self.logger_state.transition(Twe::UpKey),
                    KC::Down => self.logger_state.transition(Twe::DownKey),
                    KC::PageDown => self.logger_state.transition(Twe::NextPageKey),
                    KC::PageUp => self.logger_state.transition(Twe::PrevPageKey),
                    KC::Left => self.logger_state.transition(Twe::LeftKey),
                    KC::Right => self.logger_state.transition(Twe::RightKey),
                    _ => {}
                }
            }
        }
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

impl Default for App {
    fn default() -> Self {
        App {
            logger_state: TuiWidgetState::new(),
        }
    }
}
