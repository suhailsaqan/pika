use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};
use std::thread;

use flume::Sender;
use pika_core::{AppAction, AppReconciler, AppState, AppUpdate, AuthState, FfiApp, Screen};

#[derive(Clone)]
pub struct AppManager {
    inner: Arc<Inner>,
}

impl std::hash::Hash for AppManager {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        std::ptr::hash(Arc::as_ptr(&self.inner), state);
    }
}

struct Inner {
    core: Arc<FfiApp>,
    model: RwLock<ManagerModel>,
    nsec_store: FileNsecStore,
    subscribers: Mutex<Vec<Sender<()>>>,
}

struct ManagerModel {
    state: AppState,
    last_rev_applied: u64,
    is_restoring_session: bool,
    pending_login_nsec: Option<String>,
}

impl ManagerModel {
    fn new(initial: AppState) -> Self {
        Self {
            last_rev_applied: initial.rev,
            state: initial,
            is_restoring_session: false,
            pending_login_nsec: None,
        }
    }

    fn apply_update(&mut self, update: AppUpdate, nsec_store: &FileNsecStore) -> bool {
        let update_rev = match &update {
            AppUpdate::FullState(state) => state.rev,
            AppUpdate::AccountCreated { rev, .. } => *rev,
        };

        // Side-effect updates must not be dropped, even if stale.
        if let AppUpdate::AccountCreated { nsec, .. } = &update {
            if nsec_store.get_nsec().unwrap_or_default().is_empty() && !nsec.is_empty() {
                nsec_store.set_nsec(nsec);
            }
        }

        if update_rev <= self.last_rev_applied {
            return false;
        }

        self.last_rev_applied = update_rev;
        match update {
            AppUpdate::FullState(state) => {
                if matches!(state.auth, AuthState::LoggedIn { .. }) {
                    if let Some(nsec) = self.pending_login_nsec.take() {
                        nsec_store.set_nsec(&nsec);
                    }
                } else if state.toast.as_deref().is_some_and(|msg| {
                    msg.starts_with("Invalid nsec:")
                        || msg.starts_with("Login failed:")
                        || msg == "Enter an nsec"
                }) {
                    self.pending_login_nsec = None;
                }

                if self.is_restoring_session
                    && (!matches!(state.auth, AuthState::LoggedOut)
                        || state.router.default_screen != Screen::Login
                        || state.toast.is_some())
                {
                    self.is_restoring_session = false;
                }
                self.state = state;
            }
            AppUpdate::AccountCreated { rev, nsec, .. } => {
                if !nsec.is_empty() {
                    nsec_store.set_nsec(&nsec);
                }
                self.pending_login_nsec = None;
                self.state.rev = rev;
            }
        }

        true
    }
}

impl AppManager {
    pub fn new() -> std::io::Result<Self> {
        let data_dir = resolve_data_dir()?;
        ensure_default_config(&data_dir)?;

        let nsec_store = FileNsecStore::new(data_dir.join("desktop_nsec.txt"));
        let core = FfiApp::new(data_dir.to_string_lossy().to_string(), String::new());
        let initial = core.state();

        let inner = Arc::new(Inner {
            core: core.clone(),
            model: RwLock::new(ManagerModel::new(initial)),
            nsec_store,
            subscribers: Mutex::new(Vec::new()),
        });

        let (update_tx, update_rx) = flume::unbounded::<AppUpdate>();
        core.listen_for_updates(Box::new(ChannelReconciler { tx: update_tx }));

        let inner_for_thread = inner.clone();
        thread::spawn(move || {
            while let Ok(update) = update_rx.recv() {
                inner_for_thread.apply_update(update);
            }
        });

        if let Some(nsec) = inner.nsec_store.get_nsec() {
            if !nsec.is_empty() {
                {
                    let mut model = write_model(&inner.model);
                    model.is_restoring_session = true;
                }
                inner.core.dispatch(AppAction::RestoreSession { nsec });
            }
        }

        Ok(Self { inner })
    }

    pub fn state(&self) -> AppState {
        read_model(&self.inner.model).state.clone()
    }

    pub fn is_restoring_session(&self) -> bool {
        read_model(&self.inner.model).is_restoring_session
    }

    pub fn dispatch(&self, action: AppAction) {
        self.inner.core.dispatch(action);
    }

    pub fn subscribe_updates(&self) -> flume::Receiver<()> {
        self.inner.subscribe_updates()
    }

    pub fn login_with_nsec(&self, nsec: String) {
        let nsec = nsec.trim().to_string();
        {
            let mut model = write_model(&self.inner.model);
            model.pending_login_nsec = if nsec.is_empty() {
                None
            } else {
                Some(nsec.clone())
            };
        }
        self.inner.core.dispatch(AppAction::Login { nsec });
    }

    pub fn logout(&self) {
        self.inner.nsec_store.clear();
        self.inner.core.dispatch(AppAction::Logout);
    }
}

impl Inner {
    fn apply_update(&self, update: AppUpdate) {
        let mut model = write_model(&self.model);
        let changed = model.apply_update(update, &self.nsec_store);
        drop(model);

        if changed {
            self.notify_subscribers();
        }
    }

    fn subscribe_updates(&self) -> flume::Receiver<()> {
        let (tx, rx) = flume::unbounded();
        let mut subscribers = lock_subscribers(&self.subscribers);
        subscribers.push(tx);
        rx
    }

    fn notify_subscribers(&self) {
        let mut subscribers = lock_subscribers(&self.subscribers);
        subscribers.retain(|tx| tx.send(()).is_ok());
    }
}

struct ChannelReconciler {
    tx: Sender<AppUpdate>,
}

impl AppReconciler for ChannelReconciler {
    fn reconcile(&self, update: AppUpdate) {
        let _ = self.tx.send(update);
    }
}

#[derive(Clone)]
struct FileNsecStore {
    path: PathBuf,
}

impl FileNsecStore {
    fn new(path: PathBuf) -> Self {
        Self { path }
    }

    fn get_nsec(&self) -> Option<String> {
        let bytes = std::fs::read(&self.path).ok()?;
        let raw = String::from_utf8(bytes).ok()?;
        let trimmed = raw.trim().to_string();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    }

    fn set_nsec(&self, nsec: &str) {
        if nsec.trim().is_empty() {
            return;
        }
        if let Some(parent) = self.path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        #[cfg(unix)]
        {
            use std::io::Write as _;
            use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

            if let Ok(mut file) = std::fs::OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .mode(0o600)
                .open(&self.path)
            {
                let _ = file.write_all(nsec.as_bytes());
                let _ = file.sync_data();
                let _ =
                    std::fs::set_permissions(&self.path, std::fs::Permissions::from_mode(0o600));
                return;
            }
        }

        let _ = std::fs::write(&self.path, nsec.as_bytes());
    }

    fn clear(&self) {
        if self.path.exists() {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

pub(crate) fn resolve_data_dir() -> std::io::Result<PathBuf> {
    let dir = if let Some(raw) = std::env::var_os("PIKA_DESKTOP_DATA_DIR") {
        PathBuf::from(raw)
    } else if let Some(home) = std::env::var_os("HOME") {
        PathBuf::from(home).join(".pika")
    } else {
        PathBuf::from(".pika")
    };
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn ensure_default_config(data_dir: &Path) -> std::io::Result<()> {
    let path = data_dir.join("pika_config.json");
    if path.exists() {
        return Ok(());
    }
    let default = r#"{"call_moq_url":"https://us-east.moq.logos.surf/anon","call_broadcast_prefix":"pika/calls"}"#;
    std::fs::write(path, default.as_bytes())
}

fn read_model(lock: &RwLock<ManagerModel>) -> std::sync::RwLockReadGuard<'_, ManagerModel> {
    match lock.read() {
        Ok(guard) => guard,
        Err(poison) => poison.into_inner(),
    }
}

fn write_model(lock: &RwLock<ManagerModel>) -> std::sync::RwLockWriteGuard<'_, ManagerModel> {
    match lock.write() {
        Ok(guard) => guard,
        Err(poison) => poison.into_inner(),
    }
}

fn lock_subscribers(lock: &Mutex<Vec<Sender<()>>>) -> std::sync::MutexGuard<'_, Vec<Sender<()>>> {
    match lock.lock() {
        Ok(guard) => guard,
        Err(poison) => poison.into_inner(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state_with(rev: u64, logged_in: bool) -> AppState {
        let mut state = AppState::empty();
        state.rev = rev;
        state.auth = if logged_in {
            AuthState::LoggedIn {
                npub: "npub1test".to_string(),
                pubkey: "pubkey".to_string(),
            }
        } else {
            AuthState::LoggedOut
        };
        state
    }

    #[test]
    fn stale_full_state_is_dropped() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = FileNsecStore::new(tmp.path().join("nsec.txt"));

        let mut model = ManagerModel::new(state_with(5, false));
        model.last_rev_applied = 5;
        model.apply_update(AppUpdate::FullState(state_with(4, true)), &store);

        assert_eq!(model.state.rev, 5);
        assert_eq!(model.last_rev_applied, 5);
        assert!(matches!(model.state.auth, AuthState::LoggedOut));
    }

    #[test]
    fn account_created_side_effect_runs_when_stale() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = FileNsecStore::new(tmp.path().join("nsec.txt"));
        let mut model = ManagerModel::new(state_with(10, false));
        model.last_rev_applied = 10;

        model.apply_update(
            AppUpdate::AccountCreated {
                rev: 9,
                nsec: "nsec1phase1".to_string(),
                pubkey: "pubkey".to_string(),
                npub: "npub".to_string(),
            },
            &store,
        );

        assert_eq!(
            store.get_nsec().as_deref(),
            Some("nsec1phase1"),
            "stale AccountCreated should still persist nsec"
        );
        assert_eq!(model.last_rev_applied, 10);
    }

    #[test]
    fn restoring_session_clears_after_non_login_state() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = FileNsecStore::new(tmp.path().join("nsec.txt"));
        let mut model = ManagerModel::new(state_with(0, false));
        model.is_restoring_session = true;

        model.apply_update(AppUpdate::FullState(state_with(1, true)), &store);

        assert!(!model.is_restoring_session);
        assert_eq!(model.state.rev, 1);
    }

    #[test]
    fn pending_login_nsec_persists_after_successful_login() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = FileNsecStore::new(tmp.path().join("nsec.txt"));
        let mut model = ManagerModel::new(state_with(0, false));
        model.pending_login_nsec = Some("nsec1pending".to_string());

        model.apply_update(AppUpdate::FullState(state_with(1, true)), &store);

        assert_eq!(store.get_nsec().as_deref(), Some("nsec1pending"));
        assert!(model.pending_login_nsec.is_none());
    }

    #[test]
    fn pending_login_nsec_clears_after_login_error_toast() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = FileNsecStore::new(tmp.path().join("nsec.txt"));
        let mut model = ManagerModel::new(state_with(0, false));
        model.pending_login_nsec = Some("nsec1bad".to_string());

        let mut failed = state_with(1, false);
        failed.toast = Some("Invalid nsec: parse error".to_string());
        model.apply_update(AppUpdate::FullState(failed), &store);

        assert_eq!(store.get_nsec(), None);
        assert!(model.pending_login_nsec.is_none());
    }

    #[cfg(unix)]
    #[test]
    fn nsec_store_uses_owner_only_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("nsec.txt");
        let store = FileNsecStore::new(path.clone());

        store.set_nsec("nsec1secure");

        let mode = std::fs::metadata(path)
            .expect("metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
    }
}
