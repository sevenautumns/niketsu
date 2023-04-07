use iced::widget::{column, container};
use iced::{Application, Command, Element, Renderer, Subscription, Theme};
use log::info;
use uuid::Uuid;

use crate::file_table::{File, FileTable, FileTableMessage, FileTableState};
use crate::mpv::event::MpvEvent;
use crate::ws::WebSocketMessage;

#[derive(Debug)]
pub enum MainWindow {
    Startup(FileTableState),
    Running(),
}

#[derive(Debug, Clone)]
pub enum MainMessage {
    WebSocket(WebSocketMessage),
    Mpv(MpvEvent),
    User(UserMessage),
    DatabaseChanged,
    FileTable(FileTableMessage),
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
        let mut state = FileTableState::default();
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
                        FileTableMessage::FilePress(f) => {
                            info!("pressed: {f:?}");
                            state.file_press(f);
                        }
                        FileTableMessage::MouseRelease => {
                            state.mouse_release();
                            info!("released");
                        }
                        FileTableMessage::FileDoubleClick(f) => {
                            info!("double: {f:?}")
                        }
                        FileTableMessage::MoveIndicator(indicator) => {
                            state.move_indicator(indicator);
                            info!("move indicator: {indicator:?}")
                        }
                        FileTableMessage::FileMove(f, i) => {
                            info!("move file: {f:?}, {i}");
                            state.move_file(f, i);
                        }
                        FileTableMessage::FileDelete(f) => {
                            info!("delete file: {f:?}");
                            state.delete_file(f);
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
                FileTable::new(state).into()
            }
            MainWindow::Running() => todo!(),
        }
    }

    fn subscription(&self) -> Subscription<Self::Message> {
        // todo!()
        Subscription::none()
    }
}
