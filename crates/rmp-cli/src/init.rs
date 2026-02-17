use std::path::{Path, PathBuf};

use crate::cli::{human_log, json_print, CliError, JsonOk};

pub fn init(
    cwd: &Path,
    json: bool,
    verbose: bool,
    args: crate::cli::InitArgs,
) -> Result<(), CliError> {
    let include_ios = resolve_toggle(args.ios, args.no_ios, true);
    let include_android = resolve_toggle(args.android, args.no_android, true);
    let include_iced = resolve_toggle(args.iced, args.no_iced, false);
    if !include_ios && !include_android && !include_iced {
        return Err(CliError::user(
            "at least one platform must be enabled (use --ios, --android, or --iced)",
        ));
    }

    let requested = PathBuf::from(&args.name);
    let project_dir_name = requested
        .file_name()
        .and_then(|s| s.to_str())
        .map(|s| s.to_string())
        .ok_or_else(|| CliError::user("project name must be a valid path segment"))?;

    let dest = if requested.is_absolute() {
        requested.clone()
    } else {
        cwd.join(&requested)
    };

    if dest.exists() {
        return Err(CliError::user(format!(
            "destination already exists: {}",
            dest.to_string_lossy()
        )));
    }

    let org = args.org.unwrap_or_else(|| "com.example".to_string());
    validate_org(&org)?;

    let id_segment = java_identifier_segment(&project_dir_name);
    let bundle_id = args
        .bundle_id
        .unwrap_or_else(|| format!("{org}.{id_segment}"));
    let app_id = args.app_id.unwrap_or_else(|| format!("{org}.{id_segment}"));

    validate_bundle_like("bundle id", &bundle_id)?;
    validate_bundle_like("app id", &app_id)?;

    let display_name = display_name(&project_dir_name);

    // Derive Rust crate/lib name from the project name.
    let crate_name = rust_crate_name(&project_dir_name);
    let lib_name = crate_name.replace('-', "_");
    let iced_package = format!("{crate_name}_desktop_iced");

    // Kotlin package path segments from the app_id (e.g., "com.example.myapp" → "com/example/myapp").
    let kotlin_pkg = &app_id;
    let kotlin_pkg_path = kotlin_pkg.replace('.', "/");

    human_log(
        verbose,
        format!(
            "initializing project '{}' at {}",
            project_dir_name,
            dest.to_string_lossy()
        ),
    );

    std::fs::create_dir_all(&dest)
        .map_err(|e| CliError::operational(format!("failed to create destination: {e}")))?;

    // ── Root files ──────────────────────────────────────────────────────
    write_text(&dest.join(".gitignore"), &tpl_gitignore())?;
    write_text(&dest.join("Cargo.toml"), &tpl_workspace_toml(include_iced))?;
    write_text(
        &dest.join("rmp.toml"),
        &tpl_rmp_toml(
            &project_dir_name,
            &org,
            &crate_name,
            &bundle_id,
            &app_id,
            include_ios,
            include_android,
            include_iced,
            &iced_package,
        ),
    )?;
    write_text(
        &dest.join("justfile"),
        &tpl_justfile(include_ios, include_android, include_iced, &crate_name),
    )?;
    write_text(
        &dest.join("README.md"),
        &tpl_readme(&display_name, include_ios, include_android, include_iced),
    )?;

    // ── Rust core ───────────────────────────────────────────────────────
    let rust_dir = dest.join("rust");
    std::fs::create_dir_all(rust_dir.join("src"))
        .map_err(|e| CliError::operational(format!("create rust/src: {e}")))?;
    write_text(&rust_dir.join("Cargo.toml"), &tpl_rust_cargo(&crate_name))?;
    write_text(&rust_dir.join("build.rs"), "fn main() {}\n")?;
    write_text(&rust_dir.join("src/lib.rs"), &tpl_rust_lib())?;
    write_text(
        &rust_dir.join("uniffi.toml"),
        &tpl_uniffi_toml(kotlin_pkg, &lib_name),
    )?;

    // ── uniffi-bindgen ──────────────────────────────────────────────────
    let ub_dir = dest.join("uniffi-bindgen");
    std::fs::create_dir_all(ub_dir.join("src"))
        .map_err(|e| CliError::operational(format!("create uniffi-bindgen/src: {e}")))?;
    write_text(&ub_dir.join("Cargo.toml"), &tpl_uniffi_bindgen_cargo())?;
    write_text(
        &ub_dir.join("src/main.rs"),
        "fn main() {\n    uniffi::uniffi_bindgen_main()\n}\n",
    )?;

    // ── iOS ─────────────────────────────────────────────────────────────
    if include_ios {
        let ios_dir = dest.join("ios");
        let src_dir = ios_dir.join("Sources");
        let assets_dir = src_dir.join("Assets.xcassets/AppIcon.appiconset");
        std::fs::create_dir_all(&assets_dir)
            .map_err(|e| CliError::operational(format!("create ios dirs: {e}")))?;
        std::fs::create_dir_all(src_dir.join("Assets.xcassets"))
            .map_err(|e| CliError::operational(format!("create assets dir: {e}")))?;

        write_text(
            &ios_dir.join("project.yml"),
            &tpl_ios_project_yml(&bundle_id, &lib_name),
        )?;
        write_text(
            &ios_dir.join("Info.plist"),
            &tpl_ios_info_plist(&display_name),
        )?;
        write_text(
            &src_dir.join("App.swift"),
            &tpl_ios_app_swift(&display_name),
        )?;
        write_text(&src_dir.join("AppManager.swift"), &tpl_ios_app_manager())?;
        write_text(
            &src_dir.join("ContentView.swift"),
            &tpl_ios_content_view(&display_name),
        )?;
        write_text(
            &assets_dir.join("Contents.json"),
            &tpl_ios_appicon_contents(),
        )?;
        write_text(
            &src_dir.join("Assets.xcassets/Contents.json"),
            "{\"info\":{\"version\":1,\"author\":\"xcode\"}}\n",
        )?;
    }

    // ── Android ─────────────────────────────────────────────────────────
    if include_android {
        let android_dir = dest.join("android");
        let app_dir = android_dir.join("app");
        let main_dir = app_dir.join("src/main");
        let java_dir = main_dir.join(format!("java/{kotlin_pkg_path}"));
        let ui_dir = java_dir.join("ui");
        let theme_dir = ui_dir.join("theme");
        let res_dir = main_dir.join("res");
        let rust_bindings_dir = java_dir.join("rust");
        let gradle_dir = android_dir.join("gradle/wrapper");

        for d in [
            &java_dir,
            &ui_dir,
            &theme_dir,
            &rust_bindings_dir,
            &res_dir.join("values"),
            &res_dir.join("mipmap-hdpi"),
            &gradle_dir,
        ] {
            std::fs::create_dir_all(d)
                .map_err(|e| CliError::operational(format!("create android dirs: {e}")))?;
        }

        write_text(
            &android_dir.join("build.gradle.kts"),
            &tpl_android_root_gradle(),
        )?;
        write_text(
            &android_dir.join("settings.gradle.kts"),
            &tpl_android_settings_gradle(&display_name),
        )?;
        write_text(
            &android_dir.join("gradle.properties"),
            &tpl_android_gradle_properties(),
        )?;
        write_text(
            &app_dir.join("build.gradle.kts"),
            &tpl_android_app_gradle(&app_id, kotlin_pkg, &lib_name),
        )?;
        write_text(
            &main_dir.join("AndroidManifest.xml"),
            &tpl_android_manifest(kotlin_pkg, &display_name),
        )?;
        write_text(
            &java_dir.join("MainActivity.kt"),
            &tpl_android_main_activity(kotlin_pkg, &display_name),
        )?;
        write_text(
            &java_dir.join("AppManager.kt"),
            &tpl_android_app_manager(kotlin_pkg, &lib_name),
        )?;
        write_text(
            &ui_dir.join("MainApp.kt"),
            &tpl_android_main_app(kotlin_pkg, &display_name),
        )?;
        write_text(&theme_dir.join("Theme.kt"), &tpl_android_theme(kotlin_pkg))?;
        write_text(
            &res_dir.join("values/strings.xml"),
            &tpl_android_strings(&display_name),
        )?;
        write_text(&res_dir.join("values/themes.xml"), &tpl_android_themes())?;
        // Placeholder empty Kotlin file so Gradle's ensureUniffiGenerated doesn't fail
        // before bindings are generated.
        write_text(
            &rust_bindings_dir.join(format!("{lib_name}.kt")),
            &tpl_android_placeholder_bindings(kotlin_pkg, &lib_name),
        )?;

        // Minimal gradlew (just enough to exist; users normally get this from wrapper).
        write_gradlew(&android_dir)?;
    }

    // ── Desktop (ICED) ────────────────────────────────────────────────────
    if include_iced {
        let desktop_dir = dest.join("desktop/iced");
        std::fs::create_dir_all(desktop_dir.join("src"))
            .map_err(|e| CliError::operational(format!("create desktop/iced/src: {e}")))?;
        let src_dir = desktop_dir.join("src");
        write_text(
            &desktop_dir.join("Cargo.toml"),
            &tpl_desktop_iced_cargo(&iced_package, &crate_name),
        )?;
        write_text(
            &src_dir.join("app_manager.rs"),
            &tpl_desktop_iced_app_manager(&lib_name),
        )?;
        write_text(
            &src_dir.join("router_projection.rs"),
            &tpl_desktop_iced_router_projection(&lib_name),
        )?;
        write_text(&src_dir.join("ui.rs"), &tpl_desktop_iced_ui(&lib_name))?;
        write_text(
            &src_dir.join("main.rs"),
            &tpl_desktop_iced_main(&display_name, &lib_name),
        )?;
    }

    // ── Done ────────────────────────────────────────────────────────────
    if json {
        let mut platforms: Vec<&str> = vec![];
        if include_ios {
            platforms.push("ios");
        }
        if include_android {
            platforms.push("android");
        }
        if include_iced {
            platforms.push("iced");
        }
        json_print(&JsonOk {
            ok: true,
            data: serde_json::json!({
                "path": dest,
                "project": {
                    "name": project_dir_name,
                    "org": org,
                    "bundle_id": bundle_id,
                    "app_id": app_id,
                    "crate_name": crate_name,
                    "iced_package": iced_package,
                },
                "platforms": platforms,
            }),
        });
    } else {
        eprintln!("ok: initialized project at {}", dest.to_string_lossy());
        if include_ios {
            eprintln!("  ios bundle id: {bundle_id}");
        }
        if include_android {
            eprintln!("  android app id: {app_id}");
        }
        if include_iced {
            eprintln!("  desktop package: {iced_package}");
        }
        eprintln!(
            "  next: cd {} && just qa && rmp run iced",
            dest.to_string_lossy()
        );
    }

    Ok(())
}

// ── Helpers ─────────────────────────────────────────────────────────────────

fn resolve_toggle(include_flag: bool, exclude_flag: bool, default_value: bool) -> bool {
    if exclude_flag {
        false
    } else if include_flag {
        true
    } else {
        default_value
    }
}

fn write_text(path: &Path, content: &str) -> Result<(), CliError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            CliError::operational(format!("failed to create {}: {e}", parent.display()))
        })?;
    }
    std::fs::write(path, content)
        .map_err(|e| CliError::operational(format!("failed to write {}: {e}", path.display())))?;
    Ok(())
}

fn validate_org(org: &str) -> Result<(), CliError> {
    if org.trim().is_empty() || !org.contains('.') {
        return Err(CliError::user(
            "--org must be reverse-DNS style (for example: com.example)",
        ));
    }
    validate_bundle_like("org", org)
}

fn validate_bundle_like(label: &str, value: &str) -> Result<(), CliError> {
    if value.trim().is_empty() || !value.contains('.') {
        return Err(CliError::user(format!(
            "{label} must be dot-separated (for example: com.example.app)",
        )));
    }
    for seg in value.split('.') {
        if seg.is_empty()
            || !seg
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            return Err(CliError::user(format!(
                "{label} has invalid segment `{seg}` in `{value}`",
            )));
        }
    }
    Ok(())
}

fn java_identifier_segment(input: &str) -> String {
    let mut out = String::new();
    for c in input.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        }
    }
    if out.is_empty() {
        "app".to_string()
    } else if out.chars().next().unwrap().is_ascii_digit() {
        format!("app{out}")
    } else {
        out
    }
}

fn rust_crate_name(input: &str) -> String {
    let mut out = String::new();
    for c in input.chars() {
        if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
            out.push(c.to_ascii_lowercase());
        } else if c == ' ' {
            out.push('_');
        }
    }
    if out.is_empty() {
        "app_core".to_string()
    } else {
        // Ensure it ends with _core for clarity.
        if !out.ends_with("_core") && !out.ends_with("-core") {
            out.push_str("_core");
        }
        out
    }
}

fn display_name(input: &str) -> String {
    let mut parts: Vec<String> = vec![];
    for tok in input
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|s| !s.is_empty())
    {
        let mut chars = tok.chars();
        if let Some(first) = chars.next() {
            let mut part = String::new();
            part.push(first.to_ascii_uppercase());
            for ch in chars {
                part.push(ch.to_ascii_lowercase());
            }
            parts.push(part);
        }
    }
    if parts.is_empty() {
        "App".to_string()
    } else {
        parts.join(" ")
    }
}

// ── Template functions ──────────────────────────────────────────────────────

fn tpl_gitignore() -> String {
    r#"/target
.DS_Store
*.swp
*.swo
ios/Bindings/
ios/Frameworks/
ios/.build/
ios/*.xcodeproj
android/app/src/main/jniLibs/
android/.gradle/
android/build/
android/app/build/
android/local.properties
"#
    .to_string()
}

fn tpl_workspace_toml(include_iced: bool) -> String {
    let mut members = vec!["  \"rust\",", "  \"uniffi-bindgen\","];
    if include_iced {
        members.push("  \"desktop/iced\",");
    }
    format!(
        "[workspace]\nresolver = \"2\"\nmembers = [\n{}\n]\n",
        members.join("\n")
    )
}

fn tpl_rmp_toml(
    project_name: &str,
    org: &str,
    crate_name: &str,
    bundle_id: &str,
    app_id: &str,
    include_ios: bool,
    include_android: bool,
    include_iced: bool,
    iced_package: &str,
) -> String {
    let mut out = format!(
        r#"[project]
name = "{project_name}"
org = "{org}"

[core]
crate = "{crate_name}"
bindings = "uniffi"
"#
    );

    if include_ios {
        out.push_str(&format!(
            r#"
[ios]
bundle_id = "{bundle_id}"
"#
        ));
    }

    if include_android {
        out.push_str(&format!(
            r#"
[android]
app_id = "{app_id}"
"#
        ));
    }

    if include_iced {
        out.push_str(&format!(
            r#"
[desktop]
targets = ["iced"]

[desktop.iced]
package = "{iced_package}"
"#
        ));
    }

    out
}

fn tpl_justfile(
    include_ios: bool,
    include_android: bool,
    include_iced: bool,
    core_crate: &str,
) -> String {
    let mut lines = vec![
        "set shell := [\"bash\", \"-c\"]".to_string(),
        "".to_string(),
        "default:".to_string(),
        "  @just --list".to_string(),
        "".to_string(),
        "doctor:".to_string(),
        "  rmp doctor".to_string(),
        "".to_string(),
        "bindings:".to_string(),
        "  rmp bindings all".to_string(),
        "".to_string(),
        "qa:".to_string(),
        "  rmp doctor".to_string(),
        "  rmp bindings all".to_string(),
        format!("  cargo test -p {core_crate}"),
        "  cargo check".to_string(),
    ];

    if include_ios {
        lines.extend(
            ["", "run-ios:", "  rmp run ios"]
                .iter()
                .map(|s| s.to_string()),
        );
    }
    if include_android {
        lines.extend(
            ["", "run-android:", "  rmp run android"]
                .iter()
                .map(|s| s.to_string()),
        );
    }
    if include_iced {
        lines.extend(
            ["", "run-iced:", "  rmp run iced"]
                .iter()
                .map(|s| s.to_string()),
        );
    }
    lines.push("".to_string());
    lines.join("\n")
}

fn tpl_readme(
    display_name: &str,
    include_ios: bool,
    include_android: bool,
    include_iced: bool,
) -> String {
    let mut s = format!(
        r#"# {display_name}

A cross-platform app built with [RMP](https://github.com/nickthecook/rmp) (Rust Multiplatform).

## Quick Start

```bash
just qa
```

## Run

```bash
"#
    );
    if include_ios {
        s.push_str("rmp run ios\n");
    }
    if include_android {
        s.push_str("rmp run android\n");
    }
    if include_iced {
        s.push_str("rmp run iced\n");
    }
    s.push_str(
        r#"```

## Demo Notes

- Core state is shared in Rust. Mobile and desktop render different route projections:
  - `mobile_route`: stacked, single-column flow (`timeline>note:...>compose`).
  - `desktop_route`: split-shell flow (`main:selected=...:modal=...`).
- This app is a local one-feed demo: refresh inserts a stub note; publish inserts a signed local note.
"#,
    );
    s
}

fn tpl_rust_cargo(crate_name: &str) -> String {
    format!(
        r#"[package]
name = "{crate_name}"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib", "staticlib", "rlib"]

[dependencies]
flume = "0.11"
nostr = "0.44.2"
uniffi = "0.31.0"
"#
    )
}

fn tpl_rust_lib() -> String {
    r#"use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::thread;

use flume::{Receiver, Sender};
use nostr::{EventBuilder, Keys, ToBech32};

uniffi::setup_scaffolding!();

// ── State ───────────────────────────────────────────────────────────────────

#[derive(uniffi::Record, Clone, Debug)]
pub struct NoteSummary {
    pub id: String,
    pub author_npub: String,
    pub content: String,
    pub created_at: u64,
}

#[derive(uniffi::Record, Clone, Debug)]
pub struct AppState {
    pub rev: u64,
    // Legacy greeting kept so current mobile wrappers still compile in this phase.
    pub greeting: String,
    pub is_logged_in: bool,
    pub npub: Option<String>,
    pub timeline: Vec<NoteSummary>,
    pub selected_note_id: Option<String>,
    pub overlay: String,
    pub mobile_route: String,
    pub desktop_route: String,
    pub toast: Option<String>,
}

impl AppState {
    fn empty() -> Self {
        Self {
            rev: 0,
            greeting: "Hello from Rust!".to_string(),
            is_logged_in: false,
            npub: None,
            timeline: vec![],
            selected_note_id: None,
            overlay: "none".to_string(),
            mobile_route: "login".to_string(),
            desktop_route: "login".to_string(),
            toast: None,
        }
    }
}

// ── Actions & Updates ───────────────────────────────────────────────────────

#[derive(uniffi::Enum, Clone, Debug)]
pub enum AppAction {
    // Legacy action kept for current wrapper compatibility.
    SetName { name: String },
    CreateAccount,
    Login { nsec: String },
    RestoreSession { nsec: String },
    Logout,
    RefreshTimeline,
    PublishNote { content: String },
    SelectNote { note_id: String },
    DeselectNote,
    OpenCompose,
    CloseCompose,
    OpenSettings,
    CloseSettings,
    ClearToast,
}

#[derive(uniffi::Enum, Clone, Debug)]
pub enum AppUpdate {
    FullState(AppState),
}

// ── Semantic router + projections (internal) ───────────────────────────────

#[derive(Clone, Debug, PartialEq, Eq)]
enum SessionState {
    LoggedOut,
    LoggedIn { npub: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum OverlayState {
    None,
    Compose,
    Settings,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct NavState {
    session: SessionState,
    selected_note_id: Option<String>,
    overlay: OverlayState,
}

impl NavState {
    fn new() -> Self {
        Self {
            session: SessionState::LoggedOut,
            selected_note_id: None,
            overlay: OverlayState::None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum MobileRoot {
    Login,
    Timeline,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum MobileStack {
    NoteDetail(String),
    Compose,
    Settings,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct MobileRouteState {
    root: MobileRoot,
    stack: Vec<MobileStack>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum DesktopShell {
    Login,
    MainShell,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct DesktopRouteState {
    shell: DesktopShell,
    selected_note_id: Option<String>,
    modal: Option<OverlayState>,
}

fn project_mobile(nav: &NavState) -> MobileRouteState {
    let mut stack = vec![];
    let root = match nav.session {
        SessionState::LoggedOut => MobileRoot::Login,
        SessionState::LoggedIn { .. } => {
            if let Some(note_id) = nav.selected_note_id.as_ref() {
                stack.push(MobileStack::NoteDetail(note_id.clone()));
            }
            match nav.overlay {
                OverlayState::None => {}
                OverlayState::Compose => stack.push(MobileStack::Compose),
                OverlayState::Settings => stack.push(MobileStack::Settings),
            }
            MobileRoot::Timeline
        }
    };
    MobileRouteState { root, stack }
}

fn project_desktop(nav: &NavState) -> DesktopRouteState {
    match nav.session {
        SessionState::LoggedOut => DesktopRouteState {
            shell: DesktopShell::Login,
            selected_note_id: None,
            modal: None,
        },
        SessionState::LoggedIn { .. } => DesktopRouteState {
            shell: DesktopShell::MainShell,
            selected_note_id: nav.selected_note_id.clone(),
            modal: match nav.overlay {
                OverlayState::None => None,
                OverlayState::Compose => Some(OverlayState::Compose),
                OverlayState::Settings => Some(OverlayState::Settings),
            },
        },
    }
}

fn summarize_mobile(route: &MobileRouteState) -> String {
    let mut out = match route.root {
        MobileRoot::Login => "login".to_string(),
        MobileRoot::Timeline => "timeline".to_string(),
    };
    for entry in &route.stack {
        let seg = match entry {
            MobileStack::NoteDetail(note_id) => format!("note:{note_id}"),
            MobileStack::Compose => "compose".to_string(),
            MobileStack::Settings => "settings".to_string(),
        };
        out.push('>');
        out.push_str(&seg);
    }
    out
}

fn summarize_desktop(route: &DesktopRouteState) -> String {
    match route.shell {
        DesktopShell::Login => "login".to_string(),
        DesktopShell::MainShell => {
            let sel = route
                .selected_note_id
                .clone()
                .unwrap_or_else(|| "none".to_string());
            let modal = match route.modal {
                None => "none",
                Some(OverlayState::Compose) => "compose",
                Some(OverlayState::Settings) => "settings",
                Some(OverlayState::None) => "none",
            };
            format!("main:selected={sel}:modal={modal}")
        }
    }
}

fn overlay_label(overlay: &OverlayState) -> String {
    match overlay {
        OverlayState::None => "none".to_string(),
        OverlayState::Compose => "compose".to_string(),
        OverlayState::Settings => "settings".to_string(),
    }
}

fn trim_preview(s: &str, max: usize) -> String {
    let t = s.trim();
    if t.chars().count() <= max {
        t.to_string()
    } else {
        let mut out = String::new();
        for ch in t.chars().take(max.saturating_sub(1)) {
            out.push(ch);
        }
        out.push_str("...");
        out
    }
}

// ── Core model ──────────────────────────────────────────────────────────────

struct CoreState {
    app: AppState,
    nav: NavState,
    keys: Option<Keys>,
}

impl CoreState {
    fn new() -> Self {
        let mut out = Self {
            app: AppState::empty(),
            nav: NavState::new(),
            keys: None,
        };
        out.refresh_routes();
        out
    }

    fn refresh_routes(&mut self) {
        self.app.selected_note_id = self.nav.selected_note_id.clone();
        self.app.overlay = overlay_label(&self.nav.overlay);
        self.app.mobile_route = summarize_mobile(&project_mobile(&self.nav));
        self.app.desktop_route = summarize_desktop(&project_desktop(&self.nav));
        match &self.nav.session {
            SessionState::LoggedOut => {
                self.app.is_logged_in = false;
                self.app.npub = None;
            }
            SessionState::LoggedIn { npub } => {
                self.app.is_logged_in = true;
                self.app.npub = Some(npub.clone());
            }
        }
    }

    fn bump_rev(&mut self) {
        self.app.rev = self.app.rev.saturating_add(1);
    }

    fn seed_timeline_if_empty(&mut self, npub: &str) {
        if !self.app.timeline.is_empty() {
            return;
        }
        self.app.timeline.push(NoteSummary {
            id: "demo-welcome".to_string(),
            author_npub: npub.to_string(),
            content: "Welcome to the one-feed demo.".to_string(),
            created_at: 1,
        });
    }

    fn set_logged_in(&mut self, keys: Keys) {
        let pubkey = keys.public_key();
        let npub = pubkey.to_bech32().unwrap_or_else(|_| pubkey.to_hex());
        self.keys = Some(keys);
        self.nav.session = SessionState::LoggedIn { npub: npub.clone() };
        self.nav.overlay = OverlayState::None;
        self.seed_timeline_if_empty(&npub);
        self.app.greeting = format!("Logged in as {}", trim_preview(&npub, 16));
        self.app.toast = Some("Session ready (demo)".to_string());
    }

    fn add_demo_sync_note(&mut self) {
        let Some(npub) = self.app.npub.clone() else {
            self.app.toast = Some("Login required".to_string());
            return;
        };
        let id = format!("sync-{}-{}", self.app.rev.saturating_add(1), self.app.timeline.len());
        self.app.timeline.insert(
            0,
            NoteSummary {
                id,
                author_npub: npub,
                content: "Fetched latest notes (demo stub).".to_string(),
                created_at: self.app.rev.saturating_add(1),
            },
        );
        self.app.toast = Some("Timeline refreshed".to_string());
    }

    fn publish_local_note(&mut self, content: String) {
        let trimmed = content.trim().to_string();
        if trimmed.is_empty() {
            self.app.toast = Some("Cannot publish an empty note".to_string());
            return;
        }
        let Some(keys) = self.keys.as_ref() else {
            self.app.toast = Some("Login required".to_string());
            return;
        };
        match EventBuilder::text_note(trimmed).sign_with_keys(keys) {
            Ok(event) => {
                let npub = keys
                    .public_key()
                    .to_bech32()
                    .unwrap_or_else(|_| keys.public_key().to_hex());
                self.app.timeline.insert(
                    0,
                    NoteSummary {
                        id: event.id.to_hex(),
                        author_npub: npub,
                        content: event.content,
                        created_at: event.created_at.as_secs(),
                    },
                );
                self.nav.overlay = OverlayState::None;
                self.app.toast = Some("Published note (demo local insert)".to_string());
            }
            Err(e) => {
                self.app.toast = Some(format!("Publish failed: {e}"));
            }
        }
    }

    fn apply_action(&mut self, action: AppAction) {
        match action {
            AppAction::SetName { name } => {
                if name.trim().is_empty() {
                    self.app.greeting = "Hello from Rust!".to_string();
                } else {
                    self.app.greeting = format!("Hello, {}!", name.trim());
                }
            }
            AppAction::CreateAccount => {
                self.set_logged_in(Keys::generate());
            }
            AppAction::Login { nsec } | AppAction::RestoreSession { nsec } => {
                let nsec = nsec.trim();
                if nsec.is_empty() {
                    self.app.toast = Some("Enter an nsec".to_string());
                } else {
                    match Keys::parse(nsec) {
                        Ok(keys) => self.set_logged_in(keys),
                        Err(e) => self.app.toast = Some(format!("Invalid nsec: {e}")),
                    }
                }
            }
            AppAction::Logout => {
                self.keys = None;
                self.nav.session = SessionState::LoggedOut;
                self.nav.selected_note_id = None;
                self.nav.overlay = OverlayState::None;
                self.app.toast = Some("Logged out".to_string());
                self.app.greeting = "Hello from Rust!".to_string();
            }
            AppAction::RefreshTimeline => {
                self.add_demo_sync_note();
            }
            AppAction::PublishNote { content } => {
                self.publish_local_note(content);
            }
            AppAction::SelectNote { note_id } => {
                self.nav.selected_note_id = if note_id.trim().is_empty() {
                    None
                } else {
                    Some(note_id)
                };
            }
            AppAction::DeselectNote => {
                self.nav.selected_note_id = None;
            }
            AppAction::OpenCompose => {
                self.nav.overlay = OverlayState::Compose;
            }
            AppAction::CloseCompose => {
                self.nav.overlay = OverlayState::None;
            }
            AppAction::OpenSettings => {
                self.nav.overlay = OverlayState::Settings;
            }
            AppAction::CloseSettings => {
                self.nav.overlay = OverlayState::None;
            }
            AppAction::ClearToast => {
                self.app.toast = None;
            }
        }

        self.refresh_routes();
        self.bump_rev();
    }
}

// ── Callback interface ──────────────────────────────────────────────────────

#[uniffi::export(callback_interface)]
pub trait AppReconciler: Send + Sync + 'static {
    fn reconcile(&self, update: AppUpdate);
}

// ── FFI entry point ─────────────────────────────────────────────────────────

enum CoreMsg {
    Action(AppAction),
}

#[derive(uniffi::Object)]
pub struct FfiApp {
    core_tx: Sender<CoreMsg>,
    update_rx: Receiver<AppUpdate>,
    listening: AtomicBool,
    shared_state: Arc<RwLock<AppState>>,
}

#[uniffi::export]
impl FfiApp {
    #[uniffi::constructor]
    pub fn new(data_dir: String) -> Arc<Self> {
        let _ = data_dir; // reserved for future use

        let (update_tx, update_rx) = flume::unbounded();
        let (core_tx, core_rx) = flume::unbounded::<CoreMsg>();
        let shared_state = Arc::new(RwLock::new(AppState::empty()));

        let shared_for_core = shared_state.clone();
        thread::spawn(move || {
            let mut core = CoreState::new();

            // Emit initial state.
            {
                let snapshot = core.app.clone();
                match shared_for_core.write() {
                    Ok(mut g) => *g = snapshot.clone(),
                    Err(p) => *p.into_inner() = snapshot.clone(),
                }
                let _ = update_tx.send(AppUpdate::FullState(snapshot));
            }

            while let Ok(msg) = core_rx.recv() {
                match msg {
                    CoreMsg::Action(action) => {
                        core.apply_action(action);
                        let snapshot = core.app.clone();
                        match shared_for_core.write() {
                            Ok(mut g) => *g = snapshot.clone(),
                            Err(p) => *p.into_inner() = snapshot.clone(),
                        }
                        let _ = update_tx.send(AppUpdate::FullState(snapshot));
                    }
                }
            }
        });

        Arc::new(Self {
            core_tx,
            update_rx,
            listening: AtomicBool::new(false),
            shared_state,
        })
    }

    pub fn state(&self) -> AppState {
        match self.shared_state.read() {
            Ok(g) => g.clone(),
            Err(poison) => poison.into_inner().clone(),
        }
    }

    pub fn dispatch(&self, action: AppAction) {
        let _ = self.core_tx.send(CoreMsg::Action(action));
    }

    pub fn listen_for_updates(&self, reconciler: Box<dyn AppReconciler>) {
        if self
            .listening
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return;
        }

        let rx = self.update_rx.clone();
        thread::spawn(move || {
            while let Ok(update) = rx.recv() {
                reconciler.reconcile(update);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mobile_projection_logged_out_has_login_root() {
        let nav = NavState::new();
        let route = project_mobile(&nav);
        assert_eq!(route.root, MobileRoot::Login);
        assert!(route.stack.is_empty());
    }

    #[test]
    fn mobile_projection_logged_in_can_stack_note_and_compose() {
        let nav = NavState {
            session: SessionState::LoggedIn {
                npub: "npub1demo".to_string(),
            },
            selected_note_id: Some("note-1".to_string()),
            overlay: OverlayState::Compose,
        };
        let route = project_mobile(&nav);
        assert_eq!(route.root, MobileRoot::Timeline);
        assert_eq!(route.stack.len(), 2);
        assert_eq!(route.stack[0], MobileStack::NoteDetail("note-1".to_string()));
        assert_eq!(route.stack[1], MobileStack::Compose);
    }

    #[test]
    fn desktop_projection_keeps_split_view_selection() {
        let nav = NavState {
            session: SessionState::LoggedIn {
                npub: "npub1desktop".to_string(),
            },
            selected_note_id: Some("abc".to_string()),
            overlay: OverlayState::Settings,
        };
        let route = project_desktop(&nav);
        assert_eq!(route.shell, DesktopShell::MainShell);
        assert_eq!(route.selected_note_id, Some("abc".to_string()));
        assert_eq!(route.modal, Some(OverlayState::Settings));
    }

    #[test]
    fn logged_out_projection_hides_overlay_and_selection() {
        let nav = NavState {
            session: SessionState::LoggedOut,
            selected_note_id: Some("hidden".to_string()),
            overlay: OverlayState::Compose,
        };
        let mobile = project_mobile(&nav);
        let desktop = project_desktop(&nav);
        assert_eq!(mobile.root, MobileRoot::Login);
        assert!(mobile.stack.is_empty());
        assert_eq!(desktop.shell, DesktopShell::Login);
        assert_eq!(desktop.selected_note_id, None);
        assert_eq!(desktop.modal, None);
    }

    #[test]
    fn reducer_updates_routes_and_rev_monotonically() {
        let mut core = CoreState::new();
        assert_eq!(core.app.rev, 0);

        core.apply_action(AppAction::OpenCompose);
        assert_eq!(core.app.rev, 1);
        assert_eq!(core.app.overlay, "compose");
        assert_eq!(core.app.mobile_route, "login");
        assert_eq!(core.app.desktop_route, "login");

        core.apply_action(AppAction::CreateAccount);
        assert_eq!(core.app.rev, 2);
        assert!(core.app.is_logged_in);
        assert!(core.app.npub.is_some());

        core.apply_action(AppAction::SelectNote {
            note_id: "demo-welcome".to_string(),
        });
        assert_eq!(core.app.rev, 3);
        assert_eq!(core.app.selected_note_id, Some("demo-welcome".to_string()));
        assert!(core.app.mobile_route.contains("note:demo-welcome"));
        assert!(core.app.desktop_route.contains("selected=demo-welcome"));
    }

    #[test]
    fn logout_clears_nav_and_routes() {
        let mut core = CoreState::new();
        core.apply_action(AppAction::CreateAccount);
        core.apply_action(AppAction::OpenSettings);
        core.apply_action(AppAction::SelectNote {
            note_id: "demo-welcome".to_string(),
        });
        core.apply_action(AppAction::Logout);

        assert!(!core.app.is_logged_in);
        assert!(core.app.npub.is_none());
        assert!(core.app.selected_note_id.is_none());
        assert_eq!(core.app.overlay, "none");
        assert_eq!(core.app.mobile_route, "login");
        assert_eq!(core.app.desktop_route, "login");
    }
}
"#
    .to_string()
}

fn tpl_uniffi_toml(kotlin_pkg: &str, lib_name: &str) -> String {
    format!(
        r#"[bindings.kotlin]
package_name = "{kotlin_pkg}.rust"
cdylib_name = "{lib_name}"
"#
    )
}

fn tpl_uniffi_bindgen_cargo() -> String {
    r#"[package]
name = "uniffi-bindgen"
version = "0.1.0"
edition = "2021"
publish = false

[dependencies]
uniffi = { version = "0.31.0", features = ["cli"] }
"#
    .to_string()
}

// ── Desktop (ICED) templates ───────────────────────────────────────────────

fn tpl_desktop_iced_cargo(package: &str, core_crate: &str) -> String {
    format!(
        r#"[package]
name = "{package}"
version = "0.1.0"
edition = "2021"
publish = false

[dependencies]
{core_crate} = {{ path = "../../rust" }}
flume = "0.11"
iced = {{ version = "0.13", features = ["tokio"] }}
"#
    )
}

fn tpl_desktop_iced_main(display_name: &str, core_lib: &str) -> String {
    format!(
        r#"mod app_manager;
mod router_projection;
mod ui;

use app_manager::AppManager;
use iced::{{Subscription, Task}};
use router_projection::project_desktop;
use {core_lib} as core;

fn main() -> iced::Result {{
    iced::application("{display_name} (ICED)", DesktopApp::update, DesktopApp::view)
        .subscription(DesktopApp::subscription)
        .run_with(|| (DesktopApp::new(), Task::none()))
}}

pub struct DesktopApp {{
    manager: AppManager,
    login_nsec: String,
    compose_text: String,
}}

#[derive(Debug, Clone)]
pub enum Message {{
    Tick,
    LoginNsecChanged(String),
    ComposeChanged(String),
    CreateAccount,
    Login,
    Logout,
    Refresh,
    OpenCompose,
    CloseCompose,
    PublishCompose,
    SelectNote(String),
    DeselectNote,
    OpenSettings,
    CloseSettings,
    ClearToast,
}}

impl DesktopApp {{
    fn new() -> Self {{
        let mut manager = AppManager::new();
        let _ = manager.drain_updates();
        Self {{
            manager,
            login_nsec: String::new(),
            compose_text: String::new(),
        }}
    }}

    fn subscription(&self) -> Subscription<Message> {{
        iced::time::every(std::time::Duration::from_millis(120)).map(|_| Message::Tick)
    }}

    fn update(&mut self, message: Message) -> Task<Message> {{
        match message {{
            Message::Tick => {{
                let _ = self.manager.drain_updates();
            }}
            Message::LoginNsecChanged(value) => {{
                self.login_nsec = value;
            }}
            Message::ComposeChanged(value) => {{
                self.compose_text = value;
            }}
            Message::CreateAccount => {{
                self.dispatch(core::AppAction::CreateAccount);
            }}
            Message::Login => {{
                self.dispatch(core::AppAction::Login {{
                    nsec: self.login_nsec.clone(),
                }});
            }}
            Message::Logout => {{
                self.dispatch(core::AppAction::Logout);
            }}
            Message::Refresh => {{
                self.dispatch(core::AppAction::RefreshTimeline);
            }}
            Message::OpenCompose => {{
                self.dispatch(core::AppAction::OpenCompose);
            }}
            Message::CloseCompose => {{
                self.dispatch(core::AppAction::CloseCompose);
            }}
            Message::PublishCompose => {{
                let was_compose_open = self.manager.state.overlay == "compose";
                self.dispatch(core::AppAction::PublishNote {{
                    content: self.compose_text.clone(),
                }});
                if was_compose_open && self.manager.state.overlay != "compose" {{
                    self.compose_text.clear();
                }}
            }}
            Message::SelectNote(note_id) => {{
                self.dispatch(core::AppAction::SelectNote {{ note_id }});
            }}
            Message::DeselectNote => {{
                self.dispatch(core::AppAction::DeselectNote);
            }}
            Message::OpenSettings => {{
                self.dispatch(core::AppAction::OpenSettings);
            }}
            Message::CloseSettings => {{
                self.dispatch(core::AppAction::CloseSettings);
            }}
            Message::ClearToast => {{
                self.dispatch(core::AppAction::ClearToast);
            }}
        }}

        Task::none()
    }}

    fn view(&self) -> iced::Element<'_, Message> {{
        let route = project_desktop(&self.manager.state);
        ui::root_view(
            &self.manager.state,
            route,
            &self.login_nsec,
            &self.compose_text,
        )
    }}

    fn dispatch(&mut self, action: core::AppAction) {{
        self.manager.dispatch(action);
        let _ = self.manager.drain_updates();
    }}
}}
"#
    )
}

fn tpl_desktop_iced_app_manager(core_lib: &str) -> String {
    format!(
        r#"use std::sync::Arc;

use flume::{{Receiver, Sender}};
use {core_lib} as core;

struct UpdateBridge {{
    tx: Sender<core::AppUpdate>,
}}

impl core::AppReconciler for UpdateBridge {{
    fn reconcile(&self, update: core::AppUpdate) {{
        let _ = self.tx.send(update);
    }}
}}

pub struct AppManager {{
    ffi: Arc<core::FfiApp>,
    updates_rx: Receiver<core::AppUpdate>,
    pub state: core::AppState,
    last_rev: u64,
}}

impl AppManager {{
    pub fn new() -> Self {{
        let ffi = core::FfiApp::new(".".to_string());
        let (tx, rx) = flume::unbounded();
        ffi.listen_for_updates(Box::new(UpdateBridge {{ tx }}));

        let state = ffi.state();
        let last_rev = state.rev;
        Self {{
            ffi,
            updates_rx: rx,
            state,
            last_rev,
        }}
    }}

    pub fn dispatch(&self, action: core::AppAction) {{
        self.ffi.dispatch(action);
    }}

    pub fn drain_updates(&mut self) -> bool {{
        let mut changed = false;
        while let Ok(update) = self.updates_rx.try_recv() {{
            if self.apply_update(update) {{
                changed = true;
            }}
        }}
        changed
    }}

    fn apply_update(&mut self, update: core::AppUpdate) -> bool {{
        match update {{
            core::AppUpdate::FullState(next) => {{
                if next.rev <= self.last_rev {{
                    return false;
                }}
                self.last_rev = next.rev;
                self.state = next;
                true
            }}
        }}
    }}
}}
"#
    )
}

fn tpl_desktop_iced_router_projection(core_lib: &str) -> String {
    format!(
        r#"use {core_lib} as core;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopShell {{
    Login,
    Main,
}}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopOverlay {{
    None,
    Compose,
    Settings,
}}

#[derive(Debug, Clone)]
pub struct DesktopRoute {{
    pub shell: DesktopShell,
    pub selected_note_id: Option<String>,
    pub overlay: DesktopOverlay,
    pub route_label: String,
}}

pub fn project_desktop(state: &core::AppState) -> DesktopRoute {{
    let shell = if state.is_logged_in {{
        DesktopShell::Main
    }} else {{
        DesktopShell::Login
    }};

    let overlay = match state.overlay.as_str() {{
        "compose" => DesktopOverlay::Compose,
        "settings" => DesktopOverlay::Settings,
        _ => DesktopOverlay::None,
    }};

    DesktopRoute {{
        shell,
        selected_note_id: if state.is_logged_in {{
            state.selected_note_id.clone()
        }} else {{
            None
        }},
        overlay,
        route_label: state.desktop_route.clone(),
    }}
}}
"#
    )
}

fn tpl_desktop_iced_ui(core_lib: &str) -> String {
    format!(
        r#"use iced::widget::{{button, column, container, row, scrollable, text, text_input, Column}};
use iced::{{Center, Element, Fill, Length}};

use crate::router_projection::{{DesktopOverlay, DesktopRoute, DesktopShell}};
use crate::Message;
use {core_lib} as core;

pub fn root_view<'a>(
    state: &'a core::AppState,
    route: DesktopRoute,
    login_nsec: &'a str,
    compose_text: &'a str,
) -> Element<'a, Message> {{
    let body: Element<'a, Message> = match route.shell {{
        DesktopShell::Login => login_view(state, login_nsec),
        DesktopShell::Main => main_shell_view(state, &route, compose_text),
    }};

    if let Some(toast) = state.toast.as_ref() {{
        column![
            body,
            row![
                text(toast),
                button("Dismiss").on_press(Message::ClearToast),
            ]
            .spacing(8)
            .align_y(Center),
        ]
        .spacing(12)
        .padding(12)
        .into()
    }} else {{
        body
    }}
}}

fn login_view<'a>(state: &'a core::AppState, login_nsec: &'a str) -> Element<'a, Message> {{
    container(
        column![
            text("Desktop One-Feed Demo").size(28),
            text("Desktop shell: split timeline + detail, with modal overlays."),
            text_input("Paste nsec...", login_nsec).on_input(Message::LoginNsecChanged),
            row![
                button("Create Account").on_press(Message::CreateAccount),
                button("Login").on_press(Message::Login),
            ]
            .spacing(8),
            text(format!("rev: {{}}", state.rev)),
        ]
        .spacing(12)
        .max_width(560),
    )
    .center(Fill)
    .into()
}}

fn main_shell_view<'a>(
    state: &'a core::AppState,
    route: &DesktopRoute,
    compose_text: &'a str,
) -> Element<'a, Message> {{
    let npub = state.npub.as_deref().unwrap_or("unknown");

    let sidebar = container(
        column![
            text("Account").size(20),
            text(short(npub, 26)),
            text(format!("mobile: {{}}", state.mobile_route)),
            text(format!("route: {{}}", route.route_label)),
            row![
                button("Refresh").on_press(Message::Refresh),
                button("Compose").on_press(Message::OpenCompose),
            ]
            .spacing(8),
            row![
                button("Settings").on_press(Message::OpenSettings),
                button("Logout").on_press(Message::Logout),
            ]
            .spacing(8),
        ]
        .spacing(10),
    )
    .width(Length::FillPortion(1))
    .padding(12);

    let mut timeline_col = Column::new().spacing(8);
    if state.timeline.is_empty() {{
        timeline_col = timeline_col.push(text("Timeline is empty."));
    }} else {{
        for note in &state.timeline {{
            let mut label = short(&note.content, 52);
            if route.selected_note_id.as_deref() == Some(note.id.as_str()) {{
                label = String::from("* ") + &label;
            }}
            timeline_col =
                timeline_col.push(button(text(label)).on_press(Message::SelectNote(note.id.clone())));
        }}
    }}

    let timeline = container(
        column![
            text("Timeline").size(20),
            scrollable(timeline_col).height(Fill),
        ]
        .spacing(10),
    )
    .width(Length::FillPortion(2))
    .padding(12);

    let detail_body = if let Some(selected_id) = route.selected_note_id.as_ref() {{
        if let Some(note) = state.timeline.iter().find(|n| &n.id == selected_id) {{
            column![
                text("Note Detail").size(20),
                text(format!("id: {{}}", note.id)),
                text(format!("author: {{}}", short(&note.author_npub, 20))),
                text(&note.content),
                button("Close Detail").on_press(Message::DeselectNote),
            ]
            .spacing(10)
        }} else {{
            column![
                text("Note Detail").size(20),
                text("Selected note was not found."),
                button("Clear Selection").on_press(Message::DeselectNote),
            ]
            .spacing(10)
        }}
    }} else {{
        column![
            text("Note Detail").size(20),
            text("Select a note from the timeline."),
        ]
        .spacing(10)
    }};

    let mut detail_col = Column::new().spacing(12).push(detail_body);
    match route.overlay {{
        DesktopOverlay::Compose => {{
            detail_col = detail_col.push(
                column![
                    text("Compose").size(20),
                    text_input("Write a short note...", compose_text)
                        .on_input(Message::ComposeChanged)
                        .on_submit(Message::PublishCompose),
                    row![
                        button("Publish").on_press(Message::PublishCompose),
                        button("Cancel").on_press(Message::CloseCompose),
                    ]
                    .spacing(8),
                ]
                .spacing(8),
            );
        }}
        DesktopOverlay::Settings => {{
            detail_col = detail_col.push(
                column![
                    text("Settings").size(20),
                    text("No settings in this demo."),
                    button("Close Settings").on_press(Message::CloseSettings),
                ]
                .spacing(8),
            );
        }}
        DesktopOverlay::None => {{}}
    }}

    let detail = container(detail_col)
        .width(Length::FillPortion(2))
        .padding(12);

    row![sidebar, timeline, detail]
        .height(Fill)
        .spacing(8)
        .into()
}}

fn short(value: &str, max_chars: usize) -> String {{
    if value.chars().count() <= max_chars {{
        return value.to_string();
    }}

    let mut out = String::new();
    for ch in value.chars().take(max_chars.saturating_sub(3)) {{
        out.push(ch);
    }}
    out.push_str("...");
    out
}}
"#
    )
}

// ── iOS templates ───────────────────────────────────────────────────────────

fn tpl_ios_project_yml(bundle_id: &str, lib_name: &str) -> String {
    // The Xcode project and target are always called "App" — neutral, no renaming needed.
    // The xcframework name derives from the lib name using PascalCase.
    let xcf_name = pascal_case(lib_name);
    format!(
        r#"name: App
options:
  bundleIdPrefix: {bundle_id}
  deploymentTarget:
    iOS: "17.0"
  createIntermediateGroups: true

settings:
  base:
    PRODUCT_BUNDLE_IDENTIFIER: {bundle_id}
    MARKETING_VERSION: 0.1.0
    CURRENT_PROJECT_VERSION: 1
    SWIFT_VERSION: 5.0
  configs:
    Debug:
      PRODUCT_BUNDLE_IDENTIFIER: {bundle_id}.dev

targets:
  App:
    type: application
    platform: iOS
    sources:
      - path: Sources
      - path: Bindings
        excludes:
          - "*.h"
          - "*.modulemap"
    settings:
      base:
        INFOPLIST_FILE: Info.plist
        ASSETCATALOG_COMPILER_APPICON_NAME: AppIcon
    dependencies:
      - framework: Frameworks/{xcf_name}.xcframework
        embed: false

schemes:
  App:
    build:
      targets:
        App: all
"#
    )
}

fn tpl_ios_info_plist(display_name: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>CFBundleDevelopmentRegion</key>
	<string>en</string>
	<key>CFBundleDisplayName</key>
	<string>{display_name}</string>
	<key>CFBundleExecutable</key>
	<string>$(EXECUTABLE_NAME)</string>
	<key>CFBundleIdentifier</key>
	<string>$(PRODUCT_BUNDLE_IDENTIFIER)</string>
	<key>CFBundleName</key>
	<string>$(PRODUCT_NAME)</string>
	<key>CFBundlePackageType</key>
	<string>APPL</string>
	<key>CFBundleShortVersionString</key>
	<string>$(MARKETING_VERSION)</string>
	<key>CFBundleVersion</key>
	<string>$(CURRENT_PROJECT_VERSION)</string>
	<key>UILaunchScreen</key>
	<dict/>
	<key>UISupportedInterfaceOrientations</key>
	<array>
		<string>UIInterfaceOrientationPortrait</string>
	</array>
</dict>
</plist>
"#
    )
}

fn tpl_ios_app_swift(display_name: &str) -> String {
    format!(
        r#"import SwiftUI

@main
struct {entry_name}: App {{
    @State private var manager = AppManager()

    var body: some Scene {{
        WindowGroup {{
            ContentView(manager: manager)
        }}
    }}
}}
"#,
        entry_name = swift_app_entry_name(display_name),
    )
}

fn tpl_ios_app_manager() -> String {
    r#"import Foundation
import Observation

@MainActor
@Observable
final class AppManager: AppReconciler {
    let rust: FfiApp
    var state: AppState
    private var lastRevApplied: UInt64

    init() {
        let fm = FileManager.default
        let dataDirUrl = fm.urls(for: .applicationSupportDirectory, in: .userDomainMask).first!
        let dataDir = dataDirUrl.path
        try? fm.createDirectory(at: dataDirUrl, withIntermediateDirectories: true)

        let rust = FfiApp(dataDir: dataDir)
        self.rust = rust

        let initial = rust.state()
        self.state = initial
        self.lastRevApplied = initial.rev

        rust.listenForUpdates(reconciler: self)
    }

    nonisolated func reconcile(update: AppUpdate) {
        Task { @MainActor [weak self] in
            self?.apply(update: update)
        }
    }

    private func apply(update: AppUpdate) {
        switch update {
        case .fullState(let s):
            if s.rev <= lastRevApplied { return }
            lastRevApplied = s.rev
            state = s
        }
    }

    func dispatch(_ action: AppAction) {
        rust.dispatch(action: action)
    }
}
"#
    .to_string()
}

fn tpl_ios_content_view(display_name: &str) -> String {
    format!(
        r#"import SwiftUI

struct ContentView: View {{
    @Bindable var manager: AppManager
    @State private var loginNsec = ""
    @State private var composeInput = ""

    var body: some View {{
        NavigationStack {{
            ZStack(alignment: .bottom) {{
                if manager.state.isLoggedIn {{
                    mainFlow
                }} else {{
                    loginFlow
                }}

                if let toast = manager.state.toast {{
                    HStack(spacing: 12) {{
                        Text(toast)
                            .font(.footnote)
                        Spacer()
                        Button("Dismiss") {{
                            manager.dispatch(.clearToast)
                        }}
                        .buttonStyle(.borderedProminent)
                    }}
                    .padding(12)
                    .background(.ultraThinMaterial, in: RoundedRectangle(cornerRadius: 14))
                    .padding(.horizontal, 16)
                    .padding(.bottom, 16)
                }}
            }}

            .navigationTitle("{display_name}")
            .navigationBarTitleDisplayMode(.inline)
        }}
        .animation(.easeInOut(duration: 0.2), value: manager.state.rev)
    }}

    @ViewBuilder
    private var loginFlow: some View {{
        VStack(alignment: .leading, spacing: 12) {{
            Text("Mobile Route: \(manager.state.mobileRoute)")
                .font(.caption)
                .foregroundStyle(.secondary)
            Text("One-Feed Demo")
                .font(.title2.weight(.semibold))
            TextField("Paste nsec (optional)", text: $loginNsec)
                .textFieldStyle(.roundedBorder)
                .textInputAutocapitalization(.never)
                .autocorrectionDisabled(true)
            HStack {{
                Button("Create Account") {{
                    manager.dispatch(.createAccount)
                }}
                .buttonStyle(.borderedProminent)

                Button("Login") {{
                    manager.dispatch(.login(nsec: loginNsec))
                }}
                .buttonStyle(.bordered)
            }}
        }}
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .padding(16)
    }}

    @ViewBuilder
    private var mainFlow: some View {{
        if manager.state.overlay == "compose" {{
            composeScreen
        }} else if manager.state.overlay == "settings" {{
            settingsScreen
        }} else if let note = selectedNote {{
            noteDetail(note)
        }} else {{
            timelineScreen
        }}
    }}

    @ViewBuilder
    private var timelineScreen: some View {{
        VStack(alignment: .leading, spacing: 12) {{
            HStack {{
                Text(short(manager.state.npub ?? "unknown", max: 28))
                    .font(.caption.monospaced())
                Spacer()
                Button("Refresh") {{ manager.dispatch(.refreshTimeline) }}
                Button("Compose") {{ manager.dispatch(.openCompose) }}
                Button("Settings") {{ manager.dispatch(.openSettings) }}
                Button("Logout") {{ manager.dispatch(.logout) }}
            }}
            .buttonStyle(.bordered)

            Text("Mobile Route: \(manager.state.mobileRoute)")
                .font(.caption)
                .foregroundStyle(.secondary)

            if manager.state.timeline.isEmpty {{
                Text("Timeline is empty.")
                    .foregroundStyle(.secondary)
            }} else {{
                ScrollView {{
                    LazyVStack(alignment: .leading, spacing: 8) {{
                        ForEach(manager.state.timeline, id: \.id) {{ note in
                            Button {{
                                manager.dispatch(.selectNote(noteId: note.id))
                            }} label: {{
                                VStack(alignment: .leading, spacing: 4) {{
                                    Text(short(note.content, max: 100))
                                        .font(.body)
                                    Text(short(note.authorNpub, max: 24))
                                        .font(.caption.monospaced())
                                        .foregroundStyle(.secondary)
                                }}
                                .frame(maxWidth: .infinity, alignment: .leading)
                                .padding(10)
                                .background(.thinMaterial, in: RoundedRectangle(cornerRadius: 10))
                            }}
                            .buttonStyle(.plain)
                        }}
                    }}
                }}
            }}
        }}
        .padding(16)
    }}

    @ViewBuilder
    private func noteDetail(_ note: NoteSummary) -> some View {{
        VStack(alignment: .leading, spacing: 12) {{
            Text("Note Detail")
                .font(.title3.weight(.semibold))
            Text("id: \(note.id)")
                .font(.caption.monospaced())
                .foregroundStyle(.secondary)
            Text("author: \(short(note.authorNpub, max: 30))")
                .font(.caption)
                .foregroundStyle(.secondary)
            ScrollView {{
                Text(note.content)
                    .frame(maxWidth: .infinity, alignment: .leading)
            }}
            Button("Back to Timeline") {{
                manager.dispatch(.deselectNote)
            }}
            .buttonStyle(.bordered)
        }}
        .padding(16)
    }}

    @ViewBuilder
    private var composeScreen: some View {{
        VStack(alignment: .leading, spacing: 12) {{
            Text("Compose")
                .font(.title3.weight(.semibold))
            TextEditor(text: $composeInput)
                .frame(minHeight: 180)
                .padding(8)
                .background(.thinMaterial, in: RoundedRectangle(cornerRadius: 10))
            HStack {{
                Button("Publish") {{
                    manager.dispatch(.publishNote(content: composeInput))
                }}
                .buttonStyle(.borderedProminent)
                Button("Cancel") {{
                    manager.dispatch(.closeCompose)
                }}
                .buttonStyle(.bordered)
            }}
        }}
        .padding(16)
    }}

    @ViewBuilder
    private var settingsScreen: some View {{
        VStack(alignment: .leading, spacing: 12) {{
            Text("Settings")
                .font(.title3.weight(.semibold))
            Text("No settings in this demo.")
                .foregroundStyle(.secondary)
            Text("Mobile Route: \(manager.state.mobileRoute)")
                .font(.caption)
                .foregroundStyle(.secondary)
            Button("Close Settings") {{
                manager.dispatch(.closeSettings)
            }}
            .buttonStyle(.bordered)
        }}
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .padding(16)
    }}

    private var selectedNote: NoteSummary? {{
        guard let selected = manager.state.selectedNoteId else {{
            return nil
        }}
        return manager.state.timeline.first(where: {{ $0.id == selected }})
    }}

    private func short(_ value: String, max limit: Int) -> String {{
        guard value.count > limit else {{
            return value
        }}
        let prefix = value.prefix(Swift.max(limit - 3, 0))
        return "\(prefix)..."
    }}
}}
"#
    )
}

fn tpl_ios_appicon_contents() -> String {
    r#"{
  "images" : [
    {
      "idiom" : "universal",
      "platform" : "ios",
      "size" : "1024x1024"
    }
  ],
  "info" : {
    "author" : "xcode",
    "version" : 1
  }
}
"#
    .to_string()
}

// ── Android templates ───────────────────────────────────────────────────────

fn tpl_android_root_gradle() -> String {
    r#"plugins {
    id("com.android.application") version "8.5.1" apply false
    id("org.jetbrains.kotlin.android") version "1.9.24" apply false
}
"#
    .to_string()
}

fn tpl_android_settings_gradle(display_name: &str) -> String {
    format!(
        r#"pluginManagement {{
    repositories {{
        google()
        mavenCentral()
        gradlePluginPortal()
    }}
}}
dependencyResolutionManagement {{
    repositoriesMode.set(RepositoriesMode.FAIL_ON_PROJECT_REPOS)
    repositories {{
        google()
        mavenCentral()
    }}
}}
rootProject.name = "{display_name}"
include(":app")
"#
    )
}

fn tpl_android_gradle_properties() -> String {
    r#"android.useAndroidX=true
kotlin.code.style=official
org.gradle.jvmargs=-Xmx2048m
"#
    .to_string()
}

fn tpl_android_app_gradle(app_id: &str, kotlin_pkg: &str, lib_name: &str) -> String {
    format!(
        r#"plugins {{
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}}

android {{
    namespace = "{kotlin_pkg}"
    compileSdk = 35
    ndkVersion = "28.2.13676358"

    defaultConfig {{
        applicationId = "{app_id}"
        minSdk = 26
        targetSdk = 35
        versionCode = 1
        versionName = "0.1.0"
    }}

    buildTypes {{
        debug {{
            applicationIdSuffix = ".dev"
            versionNameSuffix = "-dev"
        }}
        release {{
            isMinifyEnabled = false
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
            )
        }}
    }}

    buildFeatures {{
        compose = true
    }}

    composeOptions {{
        kotlinCompilerExtensionVersion = "1.5.14"
    }}

    compileOptions {{
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }}

    kotlinOptions {{
        jvmTarget = "17"
    }}

    packaging {{
        resources.excludes.addAll(
            listOf("/META-INF/{{AL2.0,LGPL2.1}}", "META-INF/DEPENDENCIES"),
        )
    }}

    sourceSets {{
        getByName("main") {{
            jniLibs.srcDirs("src/main/jniLibs")
        }}
    }}
}}

tasks.register("ensureUniffiGenerated") {{
    doLast {{
        val out = file("src/main/java/{pkg_path}/rust/{lib_name}.kt")
        if (!out.exists()) {{
            throw GradleException("Missing UniFFI Kotlin bindings. Run `rmp bindings kotlin` first.")
        }}
    }}
}}

tasks.named("preBuild") {{
    dependsOn("ensureUniffiGenerated")
}}

dependencies {{
    val composeBom = platform("androidx.compose:compose-bom:2024.06.00")
    implementation(composeBom)

    implementation("androidx.core:core-ktx:1.13.1")
    implementation("androidx.activity:activity-compose:1.9.0")
    implementation("androidx.lifecycle:lifecycle-runtime-ktx:2.8.3")

    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.ui:ui-tooling-preview")
    implementation("androidx.compose.material3:material3")

    debugImplementation("androidx.compose.ui:ui-tooling")

    // UniFFI JNA
    implementation("net.java.dev.jna:jna:5.14.0@aar")
}}
"#,
        pkg_path = kotlin_pkg.replace('.', "/"),
    )
}

fn tpl_android_manifest(_kotlin_pkg: &str, display_name: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<manifest xmlns:android="http://schemas.android.com/apk/res/android">

    <uses-permission android:name="android.permission.INTERNET" />

    <application
        android:allowBackup="true"
        android:label="{display_name}"
        android:supportsRtl="true"
        android:theme="@style/Theme.App">

        <activity
            android:name=".MainActivity"
            android:exported="true">
            <intent-filter>
                <action android:name="android.intent.action.MAIN" />
                <category android:name="android.intent.category.DEFAULT" />
                <category android:name="android.intent.category.LAUNCHER" />
            </intent-filter>
        </activity>

    </application>

</manifest>
"#
    )
}

fn tpl_android_main_activity(kotlin_pkg: &str, _display_name: &str) -> String {
    format!(
        r#"package {kotlin_pkg}

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import {kotlin_pkg}.ui.MainApp
import {kotlin_pkg}.ui.theme.AppTheme

class MainActivity : ComponentActivity() {{
    private lateinit var manager: AppManager

    override fun onCreate(savedInstanceState: Bundle?) {{
        super.onCreate(savedInstanceState)
        manager = AppManager.getInstance(applicationContext)
        setContent {{
            AppTheme {{
                MainApp(manager = manager)
            }}
        }}
    }}
}}
"#
    )
}

fn tpl_android_app_manager(kotlin_pkg: &str, _lib_name: &str) -> String {
    format!(
        r#"package {kotlin_pkg}

import android.content.Context
import android.os.Handler
import android.os.Looper
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import {kotlin_pkg}.rust.AppAction
import {kotlin_pkg}.rust.AppReconciler
import {kotlin_pkg}.rust.AppState
import {kotlin_pkg}.rust.AppUpdate
import {kotlin_pkg}.rust.FfiApp

class AppManager private constructor(context: Context) : AppReconciler {{
    private val mainHandler = Handler(Looper.getMainLooper())
    private val rust: FfiApp
    private var lastRevApplied: ULong = 0UL

    var state: AppState? by mutableStateOf(null)
        private set

    init {{
        val dataDir = context.filesDir.absolutePath
        rust = FfiApp(dataDir)
        val initial = rust.state()
        state = initial
        lastRevApplied = initial.rev
        rust.listenForUpdates(this)
    }}

    fun dispatch(action: AppAction) {{
        rust.dispatch(action)
    }}

    override fun reconcile(update: AppUpdate) {{
        mainHandler.post {{
            when (update) {{
                is AppUpdate.FullState -> {{
                    if (update.v1.rev <= lastRevApplied) return@post
                    lastRevApplied = update.v1.rev
                    state = update.v1
                }}
            }}
        }}
    }}

    companion object {{
        @Volatile
        private var instance: AppManager? = null

        fun getInstance(context: Context): AppManager =
            instance ?: synchronized(this) {{
                instance ?: AppManager(context.applicationContext).also {{ instance = it }}
            }}
    }}
}}
"#
    )
}

fn tpl_android_main_app(kotlin_pkg: &str, display_name: &str) -> String {
    format!(
        r#"package {kotlin_pkg}.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import {kotlin_pkg}.AppManager
import {kotlin_pkg}.rust.AppAction
import {kotlin_pkg}.rust.AppState
import {kotlin_pkg}.rust.NoteSummary

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun MainApp(manager: AppManager) {{
    var loginNsec by rememberSaveable {{ mutableStateOf("") }}
    var composeInput by rememberSaveable {{ mutableStateOf("") }}
    val state = manager.state

    Scaffold(
        topBar = {{
            TopAppBar(title = {{ Text("{display_name}") }})
        }},
    ) {{ padding ->
        if (state == null) {{
            Box(
                modifier = Modifier
                    .fillMaxSize()
                    .padding(padding),
                contentAlignment = Alignment.Center,
            ) {{
                Text("Loading...")
            }}
            return@Scaffold
        }}

        Box(
            modifier = Modifier
                .fillMaxSize()
                .padding(padding)
        ) {{
            if (!state.isLoggedIn) {{
                LoginFlow(
                    state = state,
                    loginNsec = loginNsec,
                    onLoginNsec = {{ loginNsec = it }},
                    onCreateAccount = {{ manager.dispatch(AppAction.CreateAccount) }},
                    onLogin = {{ manager.dispatch(AppAction.Login(loginNsec)) }},
                )
            }} else {{
                val selected = state.selectedNoteId
                val selectedNote = state.timeline.firstOrNull {{ it.id == selected }}
                when {{
                    state.overlay == "compose" -> ComposeFlow(
                        composeInput = composeInput,
                        onComposeInput = {{ composeInput = it }},
                        onPublish = {{ manager.dispatch(AppAction.PublishNote(composeInput)) }},
                        onCancel = {{ manager.dispatch(AppAction.CloseCompose) }},
                    )
                    state.overlay == "settings" -> SettingsFlow(
                        state = state,
                        onClose = {{ manager.dispatch(AppAction.CloseSettings) }},
                    )
                    selectedNote != null -> DetailFlow(
                        note = selectedNote,
                        onBack = {{ manager.dispatch(AppAction.DeselectNote) }},
                    )
                    else -> TimelineFlow(
                        state = state,
                        onRefresh = {{ manager.dispatch(AppAction.RefreshTimeline) }},
                        onCompose = {{ manager.dispatch(AppAction.OpenCompose) }},
                        onSettings = {{ manager.dispatch(AppAction.OpenSettings) }},
                        onLogout = {{ manager.dispatch(AppAction.Logout) }},
                        onSelect = {{ manager.dispatch(AppAction.SelectNote(it.id)) }},
                    )
                }}
            }}

            state.toast?.let {{ toast ->
                Row(
                    modifier = Modifier
                        .align(Alignment.BottomCenter)
                        .padding(12.dp)
                        .background(
                            MaterialTheme.colorScheme.surfaceVariant,
                            shape = MaterialTheme.shapes.medium,
                        )
                        .padding(12.dp),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                ) {{
                    Text(
                        text = toast,
                        style = MaterialTheme.typography.bodySmall,
                        modifier = Modifier.weight(1f),
                    )
                    Button(onClick = {{ manager.dispatch(AppAction.ClearToast) }}) {{
                        Text("Dismiss")
                    }}
                }}
            }}
        }}
    }}
}}

@Composable
private fun LoginFlow(
    state: AppState,
    loginNsec: String,
    onLoginNsec: (String) -> Unit,
    onCreateAccount: () -> Unit,
    onLogin: () -> Unit,
) {{
    Column(
        modifier = Modifier
            .fillMaxSize()
            .padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {{
        Text(
            text = "Mobile Route: ${{state.mobileRoute}}",
            style = MaterialTheme.typography.labelSmall,
        )
        Text("One-Feed Demo", style = MaterialTheme.typography.headlineSmall)
        OutlinedTextField(
            value = loginNsec,
            onValueChange = onLoginNsec,
            label = {{ Text("Paste nsec (optional)") }},
            modifier = Modifier.fillMaxWidth(),
            singleLine = true,
        )
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {{
            Button(onClick = onCreateAccount) {{ Text("Create Account") }}
            OutlinedButton(onClick = onLogin) {{ Text("Login") }}
        }}
    }}
}}

@Composable
private fun TimelineFlow(
    state: AppState,
    onRefresh: () -> Unit,
    onCompose: () -> Unit,
    onSettings: () -> Unit,
    onLogout: () -> Unit,
    onSelect: (NoteSummary) -> Unit,
) {{
    Column(
        modifier = Modifier
            .fillMaxSize()
            .padding(12.dp),
        verticalArrangement = Arrangement.spacedBy(10.dp),
    ) {{
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {{
            OutlinedButton(onClick = onRefresh) {{ Text("Refresh") }}
            OutlinedButton(onClick = onCompose) {{ Text("Compose") }}
            OutlinedButton(onClick = onSettings) {{ Text("Settings") }}
            OutlinedButton(onClick = onLogout) {{ Text("Logout") }}
        }}
        Text(
            text = "Mobile Route: ${{state.mobileRoute}}",
            style = MaterialTheme.typography.labelSmall,
        )
        Text(
            text = short(state.npub ?: "unknown", 28),
            style = MaterialTheme.typography.labelSmall,
        )
        if (state.timeline.isEmpty()) {{
            Text("Timeline is empty.")
        }} else {{
            LazyColumn(
                modifier = Modifier.fillMaxSize(),
                verticalArrangement = Arrangement.spacedBy(8.dp),
            ) {{
                items(state.timeline) {{ note ->
                    ElevatedCard(onClick = {{ onSelect(note) }}) {{
                        Column(
                            modifier = Modifier
                                .fillMaxWidth()
                                .padding(10.dp),
                            verticalArrangement = Arrangement.spacedBy(6.dp),
                        ) {{
                            Text(short(note.content, 100))
                            Text(
                                text = short(note.authorNpub, 24),
                                style = MaterialTheme.typography.labelSmall,
                            )
                        }}
                    }}
                }}
            }}
        }}
    }}
}}

@Composable
private fun DetailFlow(note: NoteSummary, onBack: () -> Unit) {{
    Column(
        modifier = Modifier
            .fillMaxSize()
            .padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {{
        Text("Note Detail", style = MaterialTheme.typography.headlineSmall)
        Text("id: ${{note.id}}", style = MaterialTheme.typography.labelSmall)
        Text("author: ${{short(note.authorNpub, 30)}}", style = MaterialTheme.typography.labelSmall)
        Text(text = note.content, modifier = Modifier.weight(1f, fill = false))
        OutlinedButton(onClick = onBack) {{ Text("Back to Timeline") }}
    }}
}}

@Composable
private fun ComposeFlow(
    composeInput: String,
    onComposeInput: (String) -> Unit,
    onPublish: () -> Unit,
    onCancel: () -> Unit,
) {{
    Column(
        modifier = Modifier
            .fillMaxSize()
            .padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {{
        Text("Compose", style = MaterialTheme.typography.headlineSmall)
        OutlinedTextField(
            value = composeInput,
            onValueChange = onComposeInput,
            label = {{ Text("Write a short note...") }},
            modifier = Modifier
                .fillMaxWidth()
                .height(180.dp),
        )
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {{
            Button(onClick = onPublish) {{ Text("Publish") }}
            OutlinedButton(onClick = onCancel) {{ Text("Cancel") }}
        }}
    }}
}}

@Composable
private fun SettingsFlow(state: AppState, onClose: () -> Unit) {{
    Column(
        modifier = Modifier
            .fillMaxSize()
            .padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {{
        Text("Settings", style = MaterialTheme.typography.headlineSmall)
        Text("No settings in this demo.")
        Text(
            text = "Mobile Route: ${{state.mobileRoute}}",
            style = MaterialTheme.typography.labelSmall,
        )
        OutlinedButton(onClick = onClose) {{ Text("Close Settings") }}
    }}
}}

private fun short(value: String, max: Int): String {{
    return if (value.length <= max) {{
        value
    }} else {{
        value.take((max - 3).coerceAtLeast(0)) + "..."
    }}
}}
"#
    )
}

fn tpl_android_theme(kotlin_pkg: &str) -> String {
    format!(
        r#"package {kotlin_pkg}.ui.theme

import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable

private val LightColors = lightColorScheme()

@Composable
fun AppTheme(content: @Composable () -> Unit) {{
    MaterialTheme(
        colorScheme = LightColors,
        content = content,
    )
}}
"#
    )
}

fn tpl_android_strings(display_name: &str) -> String {
    format!(
        r#"<resources>
    <string name="app_name">{display_name}</string>
</resources>
"#
    )
}

fn tpl_android_themes() -> String {
    r#"<resources>
    <style name="Theme.App" parent="android:Theme.Material.Light.NoActionBar" />
</resources>
"#
    .to_string()
}

fn tpl_android_placeholder_bindings(kotlin_pkg: &str, _lib_name: &str) -> String {
    // This is a placeholder that will be overwritten by `rmp bindings kotlin`.
    // It exists so Gradle's `ensureUniffiGenerated` task doesn't block the first build.
    format!(
        r#"// Placeholder — will be replaced by `rmp bindings kotlin`.
package {kotlin_pkg}.rust
"#
    )
}

fn write_gradlew(android_dir: &Path) -> Result<(), CliError> {
    // Gradle wrapper script — minimal version that delegates to system Gradle.
    // Real projects should run `gradle wrapper` to get the full wrapper.
    let gradlew = android_dir.join("gradlew");
    write_text(
        &gradlew,
        r#"#!/bin/sh
exec gradle "$@"
"#,
    )?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&gradlew, std::fs::Permissions::from_mode(0o755));
    }
    Ok(())
}

/// Convert a snake_case lib name to PascalCase (e.g., "my_app_core" → "MyAppCore").
fn pascal_case(s: &str) -> String {
    s.split('_')
        .filter(|seg| !seg.is_empty())
        .map(|seg| {
            let mut c = seg.chars();
            match c.next() {
                Some(first) => {
                    let mut part = first.to_uppercase().to_string();
                    part.extend(c);
                    part
                }
                None => String::new(),
            }
        })
        .collect()
}

fn swift_app_entry_name(display_name: &str) -> String {
    // Turn "My App" into "MyAppApp" (SwiftUI convention).
    let cleaned: String = display_name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect();
    if cleaned.is_empty() {
        "MainApp".to_string()
    } else {
        format!("{}App", cleaned)
    }
}
