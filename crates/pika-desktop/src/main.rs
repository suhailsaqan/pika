mod app_manager;

use app_manager::AppManager;
use iced::widget::{button, column, container, row, scrollable, text, text_input};
use iced::{Center, Element, Fill, Subscription, Task};
use pika_core::{AppAction, AppState, AuthState, ChatMessage, ChatSummary};
use std::time::Duration;

pub fn main() -> iced::Result {
    iced::application("Pika Desktop (ICED)", DesktopApp::update, DesktopApp::view)
        .subscription(DesktopApp::subscription)
        .run_with(DesktopApp::new)
}

struct DesktopApp {
    manager: Option<AppManager>,
    boot_error: Option<String>,
    state: AppState,
    nsec_input: String,
    new_chat_input: String,
    message_input: String,
}

#[derive(Debug, Clone)]
enum Message {
    Tick,
    NsecChanged(String),
    Login,
    CreateAccount,
    Logout,
    NewChatChanged(String),
    StartChat,
    OpenChat(String),
    MessageChanged(String),
    SendMessage,
    ClearToast,
}

impl DesktopApp {
    fn new() -> (Self, Task<Message>) {
        match AppManager::new() {
            Ok(manager) => {
                let state = manager.state();
                (
                    Self {
                        manager: Some(manager),
                        boot_error: None,
                        state,
                        nsec_input: String::new(),
                        new_chat_input: String::new(),
                        message_input: String::new(),
                    },
                    Task::none(),
                )
            }
            Err(error) => (
                Self {
                    manager: None,
                    boot_error: Some(format!("failed to start desktop manager: {error}")),
                    state: AppState::empty(),
                    nsec_input: String::new(),
                    new_chat_input: String::new(),
                    message_input: String::new(),
                },
                Task::none(),
            ),
        }
    }

    fn subscription(&self) -> Subscription<Message> {
        if self.manager.is_some() {
            iced::time::every(Duration::from_millis(150)).map(|_| Message::Tick)
        } else {
            Subscription::none()
        }
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Tick => {
                if let Some(manager) = &self.manager {
                    let latest = manager.state();
                    if latest.rev != self.state.rev {
                        self.state = latest;
                    }
                }
            }
            Message::NsecChanged(nsec) => self.nsec_input = nsec,
            Message::Login => {
                if let Some(manager) = &self.manager {
                    manager.login_with_nsec(self.nsec_input.trim().to_string());
                }
            }
            Message::CreateAccount => {
                if let Some(manager) = &self.manager {
                    manager.dispatch(AppAction::CreateAccount);
                }
            }
            Message::Logout => {
                if let Some(manager) = &self.manager {
                    manager.logout();
                }
            }
            Message::NewChatChanged(value) => self.new_chat_input = value,
            Message::StartChat => {
                let peer_npub = self.new_chat_input.trim().to_string();
                if peer_npub.is_empty() {
                    return Task::none();
                }
                if let Some(manager) = &self.manager {
                    manager.dispatch(AppAction::CreateChat { peer_npub });
                }
                self.new_chat_input.clear();
            }
            Message::OpenChat(chat_id) => {
                if let Some(manager) = &self.manager {
                    manager.dispatch(AppAction::OpenChat { chat_id });
                }
            }
            Message::MessageChanged(value) => self.message_input = value,
            Message::SendMessage => {
                let content = self.message_input.trim().to_string();
                if content.is_empty() {
                    return Task::none();
                }
                if let Some(chat) = &self.state.current_chat {
                    if let Some(manager) = &self.manager {
                        manager.dispatch(AppAction::SendMessage {
                            chat_id: chat.chat_id.clone(),
                            content,
                        });
                    }
                    self.message_input.clear();
                }
            }
            Message::ClearToast => {
                if let Some(manager) = &self.manager {
                    manager.dispatch(AppAction::ClearToast);
                }
            }
        }

        Task::none()
    }

    fn view(&self) -> Element<'_, Message> {
        if let Some(error) = &self.boot_error {
            return container(column![text("Pika Desktop (ICED)"), text(error)].spacing(12))
                .center_x(Fill)
                .center_y(Fill)
                .into();
        }

        if matches!(self.state.auth, AuthState::LoggedOut) {
            let mut login_column = column![
                text("Pika Desktop (ICED)").size(32),
                text("Enter nsec to login"),
                text_input("nsec1...", &self.nsec_input)
                    .on_input(Message::NsecChanged)
                    .on_submit(Message::Login),
                row![
                    button("Create Account").on_press(Message::CreateAccount),
                    button("Login").on_press(Message::Login),
                ]
                .spacing(10)
            ]
            .spacing(12)
            .max_width(520)
            .align_x(Center);

            if let Some(manager) = &self.manager {
                if manager.is_restoring_session() {
                    login_column = login_column.push(text("Restoring previous session..."));
                }
            }

            return container(login_column).center_x(Fill).center_y(Fill).into();
        }

        let toast = if let Some(toast) = &self.state.toast {
            row![
                text(toast.clone()),
                button("Dismiss").on_press(Message::ClearToast),
            ]
            .spacing(8)
        } else {
            row![].spacing(0)
        };

        let chat_list = self
            .state
            .chat_list
            .iter()
            .fold(column![], |column, chat: &ChatSummary| {
                column.push(
                    button(text(chat_title(chat)))
                        .width(Fill)
                        .on_press(Message::OpenChat(chat.chat_id.clone())),
                )
            })
            .spacing(6);

        let left_rail = column![
            text("Chats").size(24),
            row![
                button("Logout").on_press(Message::Logout),
                button("Refresh").on_press(Message::Tick),
            ]
            .spacing(8),
            text_input("peer npub...", &self.new_chat_input)
                .on_input(Message::NewChatChanged)
                .on_submit(Message::StartChat),
            button("Start Chat").on_press(Message::StartChat),
            scrollable(chat_list).height(Fill),
        ]
        .spacing(10)
        .width(300)
        .padding(12);

        let chat_panel = if let Some(chat) = &self.state.current_chat {
            let messages = chat
                .messages
                .iter()
                .fold(column![], |column, msg: &ChatMessage| {
                    column.push(text(format!(
                        "{}: {}",
                        sender_label(msg),
                        msg.display_content
                    )))
                })
                .spacing(6);

            column![
                text(chat_title_from_view(
                    chat.group_name.as_deref(),
                    &chat.members
                ))
                .size(24),
                scrollable(messages).height(Fill),
                row![
                    text_input("Message...", &self.message_input)
                        .on_input(Message::MessageChanged)
                        .on_submit(Message::SendMessage),
                    button("Send").on_press(Message::SendMessage),
                ]
                .spacing(8)
            ]
            .spacing(10)
            .padding(12)
            .width(Fill)
        } else {
            column![text("Select a chat").size(24)]
                .padding(12)
                .width(Fill)
                .height(Fill)
        };

        container(column![toast, row![left_rail, chat_panel].height(Fill)].spacing(8))
            .padding(8)
            .width(Fill)
            .height(Fill)
            .into()
    }
}

fn chat_title(chat: &ChatSummary) -> String {
    if chat.is_group {
        if let Some(name) = &chat.group_name {
            return name.clone();
        }
        return "Group".to_string();
    }

    if let Some(member) = chat.members.iter().find(|member| !member.npub.is_empty()) {
        return member
            .name
            .clone()
            .unwrap_or_else(|| short_npub(&member.npub));
    }

    "Direct chat".to_string()
}

fn chat_title_from_view(group_name: Option<&str>, members: &[pika_core::MemberInfo]) -> String {
    if let Some(name) = group_name {
        if !name.trim().is_empty() {
            return name.to_string();
        }
    }
    members
        .first()
        .and_then(|member| member.name.clone())
        .unwrap_or_else(|| "Conversation".to_string())
}

fn sender_label(message: &ChatMessage) -> String {
    if message.is_mine {
        "Me".to_string()
    } else if let Some(name) = &message.sender_name {
        if !name.trim().is_empty() {
            return name.clone();
        }
        "Peer".to_string()
    } else {
        "Peer".to_string()
    }
}

fn short_npub(npub: &str) -> String {
    const TAIL: usize = 8;
    if npub.len() <= TAIL {
        return npub.to_string();
    }
    format!("...{}", &npub[npub.len() - TAIL..])
}
