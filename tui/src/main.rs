use ratatui::{style::Stylize, text::Line, widgets::Widget, DefaultTerminal, Frame};

fn main() -> std::io::Result<()>{
    let mut terminal = ratatui::init();

    let mut app = App { exit: false };

    let app_result = app.run(&mut terminal);

    ratatui::restore();
    app_result
}

pub struct App {
    exit: bool
}

impl App {
    fn run(&mut self, terminal: &mut DefaultTerminal) -> std::io::Result<()> {
        while !self.exit {
            terminal.draw(|frame| self.draw(frame))?;
        }

        Ok(())
    }

    fn draw(&self, frame: &mut Frame) {
        frame.render_widget(self, frame.area());
    }
}

impl Widget for &App {
    fn render(self, area: ratatui::prelude::Rect, buf: &mut ratatui::prelude::Buffer)
        where
            Self: Sized {
        Line::from("Process overview").bold().render(area, buf);
    }
}