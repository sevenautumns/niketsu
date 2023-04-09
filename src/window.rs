use iced::{Application, Command, Element, Renderer, Subscription, Theme};
use log::info;

use crate::file_table::{PlaylistWidget, PlaylistWidgetMessage, PlaylistWidgetState};
use crate::mpv::event::MpvEvent;
use crate::ws::WebSocketMessage;

#[derive(Debug)]
pub enum MainWindow {
    Startup(PlaylistWidgetState),
    Running(),
}

#[derive(Debug, Clone)]
pub enum MainMessage {
    WebSocket(WebSocketMessage),
    Mpv(MpvEvent),
    User(UserMessage),
    DatabaseChanged,
    FileTable(PlaylistWidgetMessage),
}

#[derive(Debug, Clone)]
pub enum UserMessage {}

impl Application for MainWindow {
    type Executor = tokio::runtime::Runtime;

    type Message = MainMessage;

    type Theme = Theme;

    type Flags = ();

    fn new(_flags: Self::Flags) -> (Self, Command<Self::Message>) {
        let files = vec![
            String::from("File 1"),
            String::from("File 2"),
            String::from("File 3"),
            String::from("File 4"),
        ];
        let mut state = PlaylistWidgetState::default();
        state.replace_files(files.clone());
        (Self::Startup(state), Command::none())
    }

    fn title(&self) -> String {
        "Sync2".into()
    }

    fn update(&mut self, message: Self::Message) -> Command<Self::Message> {
        match self {
            MainWindow::Startup(state) => {
                match message {
                    // MainMessage::WebSocket(_) => todo!(),
                    // MainMessage::Mpv(_) => todo!(),
                    // MainMessage::User(_) => todo!(),
                    // MainMessage::DatabaseChanged => todo!(),
                    MainMessage::FileTable(event) => match event {
                        PlaylistWidgetMessage::FileDoubleClick(f) => {
                            info!("double: {f:?}")
                        }
                        // PlaylistWidgetMessage::MoveIndicator(indicator) => {
                        //     state.move_indicator(indicator);
                        //     info!("move indicator: {indicator:?}")
                        // }
                        PlaylistWidgetMessage::FileMove(f, i) => {
                            info!("move file: {f:?}, {i}");
                            state.move_file(f, i);
                        }
                        PlaylistWidgetMessage::FileDelete(f) => {
                            info!("delete file: {f:?}");
                            state.delete_file(f);
                        }
                        PlaylistWidgetMessage::FileInteraction(f, i) => {
                            info!("file interaction: {f:?}, {i:?}");
                            state.file_interaction(f, i)
                        }

                        _ => {}
                    },
                    _ => {}
                }
            }
            MainWindow::Running() => todo!(),
            _ => {}
        }
        Command::none()
    }

    fn view(&self) -> Element<'_, Self::Message, Renderer<Self::Theme>> {
        // todo!()
        // container(column![].spacing(20).padding(20).max_width(600)).into()
        match self {
            MainWindow::Startup(state) => {
                //
                PlaylistWidget::new(state).into()
            }
            MainWindow::Running() => todo!(),
        }
    }

    fn subscription(&self) -> Subscription<Self::Message> {
        // todo!()
        Subscription::none()
    }
}
