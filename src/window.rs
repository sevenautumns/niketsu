use iced::{Application, Command, Element, Renderer, Subscription, Theme};
use log::info;

use crate::file_table::{PlaylistWidget, PlaylistWidgetMessage, PlaylistWidgetState};
use crate::mpv::event::MpvEvent;
use crate::mpv::Mpv;
use crate::ws::WebSocketMessage;

#[derive(Debug)]
pub enum MainWindow {
    Startup {
        url: String,
    },
    Running {
        playlist_widget: PlaylistWidgetState,
        mpv: Mpv,
    },
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
        (
            Self::Startup {
                url: String::default(),
            },
            Command::none(),
        )
    }

    fn title(&self) -> String {
        "Sync2".into()
    }

    fn update(&mut self, message: Self::Message) -> Command<Self::Message> {
        match self {
            MainWindow::Startup { url } => {}
            MainWindow::Running {
                playlist_widget,
                mpv,
            } => {
                match message {
                    // MainMessage::WebSocket(_) => todo!(),
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
                            playlist_widget.move_file(f, i);
                        }
                        PlaylistWidgetMessage::FileDelete(f) => {
                            info!("delete file: {f:?}");
                            playlist_widget.delete_file(f);
                        }
                        PlaylistWidgetMessage::FileInteraction(f, i) => {
                            info!("file interaction: {f:?}, {i:?}");
                            playlist_widget.file_interaction(f, i)
                        }
                    },
                    MainMessage::Mpv(event) => {
                        let ac = mpv.react_to(event);
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        Command::none()
    }

    fn view(&self) -> Element<'_, Self::Message, Renderer<Self::Theme>> {
        // todo!()
        // container(column![].spacing(20).padding(20).max_width(600)).into()
        match self {
            MainWindow::Startup { url } => {
                //
                // PlaylistWidget::new(state).into()
                todo!()
            }
            MainWindow::Running {
                playlist_widget,
                mpv,
            } => todo!(),
        }
    }

    fn subscription(&self) -> Subscription<Self::Message> {
        // todo!()
        Subscription::none()
    }
}
