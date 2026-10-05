//! The setup window and the commands its page calls.
//!
//! Started plainly it installs (or reinstalls, or brings an older UwUMirror up
//! to this version). `--uninstall` comes from Windows' "Installed apps" list.
//! macOS and Linux have no such list: there the setup, started again, offers
//! to uninstall.
//!
//! There is no update mode: UwUMirror 0.1 doesn't update itself (that needs
//! an update-signing key, which UwUMirror doesn't have yet). A newer setup,
//! run by hand, installs over the old version and keeps the settings.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_dialog::DialogExt;

use crate::install::{self, Installed, Layout, Options, Step};
#[cfg(windows)]
use crate::system;

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone)]
enum Mode {
    Install,
    Uninstall {
        dir: Option<PathBuf>,
        /// Windows: this is the copy in the temp folder, which deletes itself.
        #[cfg_attr(not(windows), allow(dead_code))]
        from_temp: bool,
    },
}

fn parse_mode(args: &[String]) -> Mode {
    let value_after = |flag: &str| {
        args.iter()
            .position(|a| a == flag)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    if args.iter().any(|a| a == "--uninstall") {
        Mode::Uninstall {
            dir: value_after("--dir").map(PathBuf::from),
            from_temp: args.iter().any(|a| a == "--from-temp"),
        }
    } else {
        Mode::Install
    }
}

struct Setup {
    layout: Layout,
    mode: Mode,
    /// Where the uninstaller removes UwUMirror from.
    uninstall_dir: Option<PathBuf>,
    /// Set when the page switched to uninstalling on macOS or Linux.
    uninstall_dir_override: Mutex<Option<PathBuf>>,
    busy: Mutex<bool>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Info {
    mode: &'static str,
    version: &'static str,
    installed: Option<Installed>,
    options: Options,
    app_running: bool,
    has_payload: bool,
    sandbox: bool,
    /// `windows`, `macos` or `linux`: the page words a few things differently
    /// and offers a desktop shortcut only where there is such a thing.
    platform: &'static str,
}

/// What became of the firewall rules (Windows), for the page to say.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(not(windows), allow(dead_code))]
enum Firewall {
    /// Not wanted, nothing to do, the sandbox, or not Windows.
    Untouched,
    Done,
    /// The administrator prompt was declined.
    Declined,
    Failed,
}

/// How an install or uninstall went, beyond "it worked".
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Report {
    firewall: Firewall,
    firewall_error: Option<String>,
}

impl Report {
    fn untouched() -> Self {
        Self {
            firewall: Firewall::Untouched,
            firewall_error: None,
        }
    }

    #[cfg(windows)]
    fn from_firewall(outcome: Option<Result<(), uwumirror_firewall::Error>>) -> Self {
        let (firewall, firewall_error) = match outcome {
            None => (Firewall::Untouched, None),
            Some(Ok(())) => (Firewall::Done, None),
            Some(Err(uwumirror_firewall::Error::Declined)) => (Firewall::Declined, None),
            Some(Err(error)) => (Firewall::Failed, Some(error.to_string())),
        };
        Self {
            firewall,
            firewall_error,
        }
    }
}

/// The setup window, for the administrator prompt to belong to.
#[cfg(windows)]
fn window_handle(app: &AppHandle) -> Option<isize> {
    app.get_webview_window("main")
        .and_then(|window| window.hwnd().ok())
        .map(|hwnd| hwnd.0 as isize)
}

#[derive(Clone, Serialize)]
struct ProgressEvent {
    step: Step,
    /// 0 to 1 over the whole job.
    overall: f64,
}

fn current_dir(setup: &Setup) -> PathBuf {
    if let Some(dir) = setup.uninstall_dir_override.lock().unwrap().clone() {
        return dir;
    }
    match &setup.mode {
        Mode::Uninstall { .. } => setup.uninstall_dir.clone().unwrap_or_default(),
        Mode::Install => PathBuf::from(setup.layout.remembered_options().dir),
    }
}

const PLATFORM: &str = if cfg!(windows) {
    "windows"
} else if cfg!(target_os = "macos") {
    "macos"
} else {
    "linux"
};

#[tauri::command]
fn info(setup: State<'_, Setup>) -> Info {
    let options = setup.layout.remembered_options();
    Info {
        mode: match setup.mode {
            Mode::Install => "install",
            Mode::Uninstall { .. } => "uninstall",
        },
        version: VERSION,
        installed: setup.layout.installed(),
        app_running: install::app_running(&setup.layout, &current_dir(&setup)),
        options,
        has_payload: install::has_payload(),
        sandbox: setup.layout.sandbox,
        platform: PLATFORM,
    }
}

/// Lets the user pick a folder. On Windows and Linux UwUMirror goes into a
/// "UwUMirror" folder inside it; on macOS the app goes straight in, the way
/// it goes into /Applications.
#[tauri::command]
async fn pick_folder(app: AppHandle, current: String) -> Option<String> {
    let start = Path::new(&current)
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default();
    let picked = app
        .dialog()
        .file()
        .set_directory(start)
        .blocking_pick_folder()?;
    let path = picked.into_path().ok()?;
    Some(install::folder_for(path).display().to_string())
}

#[tauri::command]
async fn close_app(app: AppHandle) -> Result<(), String> {
    let setup = app.state::<Setup>();
    install::stop_app(&setup.layout, &current_dir(&setup))
}

/// Turns step-local progress into one smooth 0..1 value and sends it to the page.
fn reporter(app: &AppHandle, weights: &'static [(Step, f64)]) -> impl FnMut(Step, f64) {
    let app = app.clone();
    let mut last = -1.0;
    move |step, fraction| {
        let mut overall = 0.0;
        for (candidate, weight) in weights {
            if *candidate == step {
                overall += weight * fraction.clamp(0.0, 1.0);
                break;
            }
            overall += weight;
        }
        let overall = if step == Step::Done {
            1.0
        } else {
            overall.min(0.99)
        };
        if overall - last >= 0.01 || step == Step::Done {
            last = overall;
            let _ = app.emit("setup:progress", ProgressEvent { step, overall });
        }
    }
}

fn guard(setup: &Setup) -> Result<BusyGuard<'_>, String> {
    let mut busy = setup.busy.lock().unwrap();
    if *busy {
        return Err("Setup is already working.".into());
    }
    *busy = true;
    Ok(BusyGuard(&setup.busy))
}

struct BusyGuard<'a>(&'a Mutex<bool>);

impl Drop for BusyGuard<'_> {
    fn drop(&mut self) {
        *self.0.lock().unwrap() = false;
    }
}

#[tauri::command]
async fn install(app: AppHandle, options: Options) -> Result<Report, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let setup = app.state::<Setup>();
        let _busy = guard(&setup)?;
        const WEIGHTS: &[(Step, f64)] = &[
            (Step::Prepare, 0.08),
            (Step::Copy, 0.72),
            (Step::Shortcuts, 0.08),
            (Step::Register, 0.12),
        ];
        let mut progress = reporter(&app, WEIGHTS);
        // "Done" waits until the firewall is set up, too.
        let mut until_done = |step: Step, fraction: f64| {
            if step != Step::Done {
                progress(step, fraction);
            }
        };
        install::install(&setup.layout, &options, VERSION, &mut until_done)?;
        // After the files: a "no" to the administrator prompt must not cost
        // the install. The app can set the firewall up later.
        #[cfg(windows)]
        let report = if options.firewall {
            let dir = PathBuf::from(options.dir.trim());
            Report::from_firewall(install::set_up_firewall(
                &setup.layout,
                &dir,
                window_handle(&app),
            ))
        } else {
            Report::untouched()
        };
        #[cfg(not(windows))]
        let report = Report::untouched();
        progress(Step::Done, 1.0);
        Ok(report)
    })
    .await
    .map_err(|e| format!("Setup stopped unexpectedly: {e}"))?
}

#[tauri::command]
async fn uninstall(app: AppHandle, keep_data: bool) -> Result<Report, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let setup = app.state::<Setup>();
        let _busy = guard(&setup)?;
        let dir = current_dir(&setup);
        const WEIGHTS: &[(Step, f64)] = &[
            (Step::Prepare, 0.15),
            (Step::Shortcuts, 0.1),
            (Step::Register, 0.15),
            (Step::Copy, 0.3),
            (Step::Cleanup, 0.3),
        ];
        let mut progress = reporter(&app, WEIGHTS);
        // The rules first, while nothing is gone yet; a "no" leaves them and
        // carries on.
        #[cfg(windows)]
        let report = Report::from_firewall(install::remove_firewall(
            &setup.layout,
            &dir,
            window_handle(&app),
        ));
        #[cfg(not(windows))]
        let report = Report::untouched();
        install::uninstall(&setup.layout, &dir, keep_data, &mut progress)?;
        Ok(report)
    })
    .await
    .map_err(|e| format!("Setup stopped unexpectedly: {e}"))?
}

#[tauri::command]
fn launch_app(setup: State<'_, Setup>) -> Result<(), String> {
    install::launch(&setup.layout, &current_dir(&setup))
}

/// macOS and Linux have no "Installed apps" list to start the uninstaller
/// from, so the setup itself offers it when UwUMirror is there.
#[tauri::command]
fn begin_uninstall(setup: State<'_, Setup>) -> Result<(), String> {
    if cfg!(windows) {
        return Err("On Windows, UwUMirror is removed from Installed apps.".into());
    }
    let dir = setup
        .layout
        .installed()
        .map(|installed| PathBuf::from(installed.dir))
        .ok_or("UwUMirror isn't installed.")?;
    *setup.uninstall_dir_override.lock().unwrap() = Some(dir);
    Ok(())
}

#[tauri::command]
fn finish(app: AppHandle) {
    #[cfg(windows)]
    {
        let setup = app.state::<Setup>();
        if let Mode::Uninstall {
            from_temp: true, ..
        } = setup.mode
        {
            if let Ok(me) = std::env::current_exe() {
                system::delete_after_exit(&me);
            }
        }
    }
    app.exit(0);
}

pub fn run() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mode = parse_mode(&args);
    let layout = Layout::detect();

    let uninstall_dir = match &mode {
        Mode::Uninstall { dir, .. } => dir
            .clone()
            .or_else(|| {
                layout
                    .installed()
                    .map(|installed| PathBuf::from(installed.dir))
            })
            .or_else(|| {
                std::env::current_exe()
                    .ok()
                    .and_then(|me| me.parent().map(Path::to_path_buf))
            }),
        Mode::Install => None,
    };

    // Windows can't delete a running program, so the uninstaller works from a
    // copy in the temp folder that removes itself at the end.
    #[cfg(windows)]
    if let (
        Mode::Uninstall {
            from_temp: false, ..
        },
        Some(dir),
    ) = (&mode, &uninstall_dir)
    {
        if let Ok(me) = std::env::current_exe() {
            let copy = std::env::temp_dir()
                .join(format!("UwUMirror-Uninstall-{}.exe", std::process::id()));
            if std::fs::copy(&me, &copy).is_ok() {
                let dir = dir.display().to_string();
                if system::spawn_detached(&copy, &["--uninstall", "--from-temp", "--dir", &dir])
                    .is_ok()
                {
                    return;
                }
            }
        }
    }

    #[cfg(windows)]
    if !system::webview2_installed() {
        let (title, text) = if system_is_german() {
            (
                "UwUMirror Setup",
                "UwUMirror braucht Microsoft Edge WebView2. Soll es jetzt heruntergeladen und installiert werden?",
            )
        } else {
            (
                "UwUMirror Setup",
                "UwUMirror needs Microsoft Edge WebView2. Download and install it now?",
            )
        };
        if !system::ask(title, text) {
            return;
        }
        if let Err(error) = system::install_webview2() {
            system::alert(title, &error);
            return;
        }
    }

    let setup = Setup {
        layout,
        mode,
        uninstall_dir,
        uninstall_dir_override: Mutex::new(None),
        busy: Mutex::new(false),
    };
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(setup)
        .setup(|app| {
            let window =
                WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                    .title("UwUMirror Setup")
                    .inner_size(460.0, 640.0)
                    .resizable(false)
                    .maximizable(false)
                    .shadow(true)
                    .center();
            // A Mac gets its traffic lights, laid over the page's own bar;
            // elsewhere the page draws its buttons in a frameless window.
            #[cfg(target_os = "macos")]
            let window = window
                .decorations(true)
                .title_bar_style(tauri::TitleBarStyle::Overlay)
                .hidden_title(true)
                .traffic_light_position(tauri::LogicalPosition::new(16.0, 16.0));
            #[cfg(not(target_os = "macos"))]
            let window = window.decorations(false);
            // The page's own browser data goes to the temp folder, not next to
            // UwUMirror's. WKWebView keeps its data by bundle id and takes no folder.
            #[cfg(not(target_os = "macos"))]
            let window =
                window.data_directory(std::env::temp_dir().join("UwUMirror-Setup-WebView"));
            window.build()?;
            Ok(())
        })
        // The red traffic light while installing: the half-copied app must
        // not be left behind, so the window stays until the step is done.
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if *window.state::<Setup>().busy.lock().unwrap() {
                    api.prevent_close();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            info,
            pick_folder,
            close_app,
            install,
            uninstall,
            launch_app,
            begin_uninstall,
            finish
        ])
        .run(tauri::generate_context!())
        .expect("error while running UwUMirror Setup");
}

#[cfg(windows)]
fn system_is_german() -> bool {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;
    RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(r"Control Panel\International")
        .and_then(|key| key.get_value::<String, _>("LocaleName"))
        .is_ok_and(|locale| locale.to_ascii_lowercase().starts_with("de"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn tells_the_page_what_became_of_the_firewall() {
        use uwumirror_firewall::Error;
        let report = |outcome| Report::from_firewall(outcome).firewall;
        assert_eq!(report(None), Firewall::Untouched);
        assert_eq!(report(Some(Ok(()))), Firewall::Done);
        assert_eq!(report(Some(Err(Error::Declined))), Firewall::Declined);
        let failed = Report::from_firewall(Some(Err(Error::Failed("exit code 1".into()))));
        assert_eq!(failed.firewall, Firewall::Failed);
        assert_eq!(failed.firewall_error.as_deref(), Some("exit code 1"));
        let json = serde_json::to_string(&Report::from_firewall(Some(Ok(())))).unwrap();
        assert_eq!(json, r#"{"firewall":"done","firewallError":null}"#);
    }

    #[test]
    fn reads_the_modes() {
        let args = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(matches!(parse_mode(&args(&[])), Mode::Install));
        // No update mode: flags the setup doesn't know start a plain install.
        assert!(matches!(
            parse_mode(&args(&["--update", "--relaunch"])),
            Mode::Install
        ));
        assert!(matches!(
            parse_mode(&args(&[
                "--uninstall",
                "--from-temp",
                "--dir",
                r"C:\Apps\UwUMirror"
            ])),
            Mode::Uninstall {
                from_temp: true,
                dir: Some(_)
            }
        ));
    }
}
