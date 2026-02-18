mod app_manager;
mod theme;
mod views;

use app_manager::AppManager;
use iced::widget::{column, container, row, rule, text};
use iced::{Element, Fill, Font, Size, Subscription, Task, Theme};
use pika_core::{AppAction, AppState, AuthState};
use std::time::Duration;

pub fn main() -> iced::Result {
    iced::application(DesktopApp::new, DesktopApp::update, DesktopApp::view)
        .title("Pika Desktop")
        .subscription(DesktopApp::subscription)
        .theme(dark_theme)
        .window_size(Size::new(1024.0, 720.0))
        .font(include_bytes!("../fonts/UbuntuSansMono.ttf").as_slice())
        .default_font(Font::with_name("Ubuntu Sans Mono"))
        .run()
}

fn dark_theme(_state: &DesktopApp) -> Theme {
    Theme::Dark
}

struct DesktopApp {
    manager: Option<AppManager>,
    boot_error: Option<String>,
    state: AppState,
    avatar_cache: std::cell::RefCell<views::avatar::AvatarCache>,
    cached_profiles: Vec<pika_core::FollowListEntry>,
    nsec_input: String,
    new_chat_input: String,
    new_chat_search: String,
    filtered_follows: Vec<pika_core::FollowListEntry>,
    message_input: String,
    show_new_chat_form: bool,
    selected_chat_id: Option<String>,
}

#[derive(Debug, Clone)]
pub enum Message {
    Tick,
    NsecChanged(String),
    Login,
    CreateAccount,
    Logout,
    NewChatChanged(String),
    NewChatSearchChanged(String),
    StartChat,
    StartChatWith(String),
    ToggleNewChatForm,
    OpenChat(String),
    MessageChanged(String),
    SendMessage,
    ClearToast,
}

impl DesktopApp {
    fn new() -> (Self, Task<Message>) {
        let data_dir = Self::data_dir();
        let cached_profiles = pika_core::load_cached_profiles(&data_dir);

        match AppManager::new() {
            Ok(manager) => {
                let state = manager.state();
                (
                    Self {
                        manager: Some(manager),
                        boot_error: None,
                        state,
                        avatar_cache: std::cell::RefCell::new(views::avatar::AvatarCache::new()),
                        cached_profiles,
                        nsec_input: String::new(),
                        new_chat_input: String::new(),
                        new_chat_search: String::new(),
                        filtered_follows: Vec::new(),
                        message_input: String::new(),
                        show_new_chat_form: false,
                        selected_chat_id: None,
                    },
                    Task::none(),
                )
            }
            Err(error) => (
                Self {
                    manager: None,
                    boot_error: Some(format!("failed to start desktop manager: {error}")),
                    state: AppState::empty(),
                    avatar_cache: std::cell::RefCell::new(views::avatar::AvatarCache::new()),
                    cached_profiles,
                    nsec_input: String::new(),
                    new_chat_input: String::new(),
                    new_chat_search: String::new(),
                    filtered_follows: Vec::new(),
                    message_input: String::new(),
                    show_new_chat_form: false,
                    selected_chat_id: None,
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
                        // On login transition, dispatch Foregrounded to load profiles
                        let was_logged_out = matches!(self.state.auth, AuthState::LoggedOut);
                        let now_logged_in = matches!(latest.auth, AuthState::LoggedIn { .. });
                        if was_logged_out && now_logged_in {
                            manager.dispatch(AppAction::Foregrounded);
                        }

                        // Close new-chat form once creating_chat finishes
                        if self.state.busy.creating_chat && !latest.busy.creating_chat {
                            self.show_new_chat_form = false;
                            self.new_chat_input.clear();
                        }

                        // Sync selected_chat_id if core's current_chat changed
                        if latest.current_chat.is_none() {
                            self.selected_chat_id = None;
                        }
                        self.state = latest;
                        if self.show_new_chat_form {
                            self.refilter_follows();
                        }
                    }
                    // Retry outside the manager borrow
                    if self.show_new_chat_form
                        && self.state.follow_list.is_empty()
                        && !self.state.busy.fetching_follow_list
                    {
                        if let Some(manager) = &self.manager {
                            manager.dispatch(AppAction::RefreshFollowList);
                        }
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
                self.avatar_cache.borrow_mut().clear();
                self.selected_chat_id = None;
                self.show_new_chat_form = false;
            }
            Message::NewChatChanged(value) => self.new_chat_input = value,
            Message::NewChatSearchChanged(value) => {
                self.new_chat_search = value;
                self.refilter_follows();
            }
            Message::ToggleNewChatForm => {
                self.show_new_chat_form = !self.show_new_chat_form;
                if !self.show_new_chat_form {
                    self.new_chat_input.clear();
                    self.new_chat_search.clear();
                }
                // Refresh follow list when opening
                if self.show_new_chat_form {
                    self.refilter_follows();
                    if let Some(manager) = &self.manager {
                        manager.dispatch(AppAction::RefreshFollowList);
                    }
                }
            }
            Message::StartChat => {
                let peer_npub = self.new_chat_input.trim().to_string();
                if peer_npub.is_empty() {
                    return Task::none();
                }
                if let Some(manager) = &self.manager {
                    manager.dispatch(AppAction::CreateChat { peer_npub });
                }
            }
            Message::StartChatWith(peer_npub) => {
                if let Some(manager) = &self.manager {
                    manager.dispatch(AppAction::CreateChat { peer_npub });
                }
                // Keep form open — it will show a loading state via busy.creating_chat.
                // Form closes when the tick detects creating_chat went false.
            }
            Message::OpenChat(chat_id) => {
                self.selected_chat_id = Some(chat_id.clone());
                self.show_new_chat_form = false;
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
        // ── Boot error ──────────────────────────────────────────────
        if let Some(error) = &self.boot_error {
            return container(
                column![
                    text("Pika Desktop")
                        .size(24)
                        .color(theme::TEXT_PRIMARY),
                    text(error).color(theme::DANGER),
                ]
                .spacing(12),
            )
            .center_x(Fill)
            .center_y(Fill)
            .style(theme::surface_style)
            .into();
        }

        // ── Login screen ────────────────────────────────────────────
        if matches!(self.state.auth, AuthState::LoggedOut) {
            let is_restoring = self
                .manager
                .as_ref()
                .map_or(false, |m| m.is_restoring_session());

            return views::login::login_view(
                &self.nsec_input,
                self.state.busy.creating_account,
                is_restoring,
                self.state.toast.as_deref(),
            );
        }

        // ── Main 3-pane layout ──────────────────────────────────────

        // Toast bar (optional)
        let mut main_column = column![];
        if let Some(toast_msg) = &self.state.toast {
            main_column = main_column.push(views::toast::toast_bar(toast_msg));
        }

        let cache = &mut *self.avatar_cache.borrow_mut();
        cache.reset_budget();

        // Chat rail (left sidebar)
        let rail = views::chat_rail::chat_rail_view(
            &self.state.chat_list,
            self.selected_chat_id.as_deref(),
            self.show_new_chat_form,
            cache,
        );

        // Center pane: new chat, conversation, or empty state
        let center_pane: Element<'_, Message> = if self.show_new_chat_form {
            views::new_chat::new_chat_view(
                &self.filtered_follows,
                &self.new_chat_input,
                self.state.busy.creating_chat,
                self.state.busy.fetching_follow_list,
                &self.new_chat_search,
                cache,
            )
        } else if let Some(chat) = &self.state.current_chat {
            views::conversation::conversation_view(chat, &self.message_input, cache)
        } else {
            views::empty_state::empty_state_view()
        };

        let content = row![rail, rule::vertical(1), center_pane].height(Fill);

        main_column = main_column.push(content);

        container(main_column)
            .width(Fill)
            .height(Fill)
            .style(theme::surface_style)
            .into()
    }

    fn data_dir() -> String {
        if let Some(raw) = std::env::var_os("PIKA_DESKTOP_DATA_DIR") {
            return raw.to_string_lossy().to_string();
        }
        if let Some(home) = std::env::var_os("HOME") {
            return std::path::Path::new(&home)
                .join(".pika-desktop")
                .to_string_lossy()
                .to_string();
        }
        ".pika-desktop".to_string()
    }

    fn refilter_follows(&mut self) {
        // Use the relay follow list if available, otherwise fall back to
        // all cached profiles from the on-disk database.
        let source = if self.state.follow_list.is_empty() {
            &self.cached_profiles
        } else {
            &self.state.follow_list
        };

        if self.new_chat_search.is_empty() {
            self.filtered_follows = source.clone();
        } else {
            let q = self.new_chat_search.to_lowercase();
            self.filtered_follows = source
                .iter()
                .filter(|e| {
                    e.name
                        .as_deref()
                        .unwrap_or("")
                        .to_lowercase()
                        .contains(&q)
                        || e.npub.to_lowercase().contains(&q)
                })
                .cloned()
                .collect();
        }
    }
}
