//! The menu bar on a Mac, as every Mac app has it: UwUMirror (About,
//! Settings… ⌘,, Services, Hide, Quit), Edit (without it ⌘C and ⌘V do
//! nothing in the page's text fields), View (Full Screen ⌃⌘F) and Window.
//! Windows and Linux have no menu bar; their window has its own title bar.
//!
//! The words follow the system's language, German or English, like the page
//! does by default; the standard items are the system's own.

use tauri::menu::{
    AboutMetadataBuilder, Menu, MenuItem, PredefinedMenuItem, Submenu, HELP_SUBMENU_ID,
    WINDOW_SUBMENU_ID,
};
use tauri::{AppHandle, Emitter, Runtime};
use tauri_plugin_opener::OpenerExt;

/// Asks the page to open its settings, as ⌘, does.
const SETTINGS: &str = "settings";
const WEBSITE: &str = "website";

/// The system's language is German: `defaults read -g AppleLocale` says
/// `de_DE` and the like. Anything else gets English.
fn german() -> bool {
    std::process::Command::new("defaults")
        .args(["read", "-g", "AppleLocale"])
        .output()
        .map(|out| {
            String::from_utf8_lossy(&out.stdout)
                .trim()
                .starts_with("de")
        })
        .unwrap_or(false)
}

pub fn build<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Menu<R>> {
    let de = german();
    let pick = |german: &'static str, english: &'static str| if de { german } else { english };

    let about = AboutMetadataBuilder::new()
        .name(Some("UwUMirror"))
        .version(Some(env!("CARGO_PKG_VERSION")))
        .copyright(Some("AGPL-3.0-only"))
        .build();
    let app_menu = Submenu::with_items(
        app,
        "UwUMirror",
        true,
        &[
            &PredefinedMenuItem::about(
                app,
                Some(pick("Über UwUMirror", "About UwUMirror")),
                Some(about),
            )?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(
                app,
                SETTINGS,
                pick("Einstellungen …", "Settings…"),
                true,
                Some("CmdOrCtrl+,"),
            )?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::services(app, Some(pick("Dienste", "Services")))?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::hide(app, Some(pick("UwUMirror ausblenden", "Hide UwUMirror")))?,
            &PredefinedMenuItem::hide_others(app, Some(pick("Andere ausblenden", "Hide Others")))?,
            &PredefinedMenuItem::show_all(app, Some(pick("Alle einblenden", "Show All")))?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::quit(app, Some(pick("UwUMirror beenden", "Quit UwUMirror")))?,
        ],
    )?;
    let edit = Submenu::with_items(
        app,
        pick("Bearbeiten", "Edit"),
        true,
        &[
            &PredefinedMenuItem::undo(app, Some(pick("Widerrufen", "Undo")))?,
            &PredefinedMenuItem::redo(app, Some(pick("Wiederholen", "Redo")))?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::cut(app, Some(pick("Ausschneiden", "Cut")))?,
            &PredefinedMenuItem::copy(app, Some(pick("Kopieren", "Copy")))?,
            &PredefinedMenuItem::paste(app, Some(pick("Einsetzen", "Paste")))?,
            &PredefinedMenuItem::select_all(app, Some(pick("Alles auswählen", "Select All")))?,
        ],
    )?;
    let view = Submenu::with_items(
        app,
        pick("Darstellung", "View"),
        true,
        &[&PredefinedMenuItem::fullscreen(
            app,
            Some(pick("Vollbildmodus", "Enter Full Screen")),
        )?],
    )?;
    // With the system's own id, so macOS adds its window list and tiling.
    let window = Submenu::with_id_and_items(
        app,
        WINDOW_SUBMENU_ID,
        pick("Fenster", "Window"),
        true,
        &[
            &PredefinedMenuItem::minimize(app, Some(pick("Im Dock ablegen", "Minimize")))?,
            &PredefinedMenuItem::maximize(app, Some(pick("Zoomen", "Zoom")))?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::close_window(
                app,
                Some(pick("Fenster schließen", "Close Window")),
            )?,
        ],
    )?;
    let help = Submenu::with_id_and_items(
        app,
        HELP_SUBMENU_ID,
        pick("Hilfe", "Help"),
        true,
        &[&MenuItem::with_id(
            app,
            WEBSITE,
            pick("UwUMirror auf GitHub", "UwUMirror on GitHub"),
            true,
            None::<&str>,
        )?],
    )?;
    Menu::with_items(app, &[&app_menu, &edit, &view, &window, &help])
}

/// What the custom items do; the predefined ones the system handles itself.
pub fn handle<R: Runtime>(app: &AppHandle<R>, id: &str) {
    match id {
        SETTINGS => {
            let _ = app.emit("menu", SETTINGS);
        }
        WEBSITE => {
            let _ = app
                .opener()
                .open_url("https://github.com/MinifyX/UwUMirror", None::<&str>);
        }
        _ => {}
    }
}
