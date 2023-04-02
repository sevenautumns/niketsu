use iced::{Application, Theme};

#[derive(Debug)]
pub enum MainWindow {
    Startup(),
    Running(),
}

#[derive(Debug, Clone)]
pub enum Message {}

impl Application for MainWindow {
    type Executor = tokio::runtime::Runtime;

    type Message = Message;

    type Theme = Theme;

    type Flags = ();

    fn new(flags: Self::Flags) -> (Self, iced::Command<Self::Message>) {
        todo!()
    }

    fn title(&self) -> String {
        todo!()
    }

    fn update(&mut self, message: Self::Message) -> iced::Command<Self::Message> {
        todo!()
    }

    fn view(&self) -> iced::Element<'_, Self::Message, iced::Renderer<Self::Theme>> {
        todo!()
    }
}
