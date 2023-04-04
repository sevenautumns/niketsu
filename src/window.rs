use iced::{
    widget::{column, container},
    Application, Command, Element, Renderer, Subscription, Theme,
};

#[derive(Debug)]
pub enum MainWindow {
    Startup(),
    Running(),
}

#[derive(Debug, Clone)]
pub enum Message {
    Server(ServerMessage),
    Mpv(MpvMessage),
    User(UserMessage),
}

#[derive(Debug, Clone)]
pub enum ServerMessage {}

#[derive(Debug, Clone)]
pub enum MpvMessage {}

#[derive(Debug, Clone)]
pub enum UserMessage {}

impl Application for MainWindow {
    type Executor = tokio::runtime::Runtime;

    type Message = Message;

    type Theme = Theme;

    type Flags = ();

    fn new(_flags: Self::Flags) -> (Self, Command<Self::Message>) {
        (Self::Running(), Command::none())
    }

    fn title(&self) -> String {
        "Sync2".into()
    }

    fn update(&mut self, _message: Self::Message) -> Command<Self::Message> {
        // todo!()
        Command::none()
    }

    fn view(&self) -> Element<'_, Self::Message, Renderer<Self::Theme>> {
        // todo!()
        container(column![].spacing(20).padding(20).max_width(600)).into()
    }

    fn subscription(&self) -> Subscription<Self::Message> {
        // todo!()
        Subscription::none()
    }
}
