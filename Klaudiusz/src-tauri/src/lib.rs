//! Klaudiusz: asystent głosowy w pasku menu.
//! Moduły odczytu, zapisów i akcji są już podpięte pod rozmowę głosową.
#![allow(dead_code)]

mod alerts;
mod apps;
mod audio;
mod cthx;
mod cthx_write;
mod dates;
mod exporter;
mod finance;
mod journal;
mod llm;
mod mail;
mod money;
mod notes;
mod notice;
mod paths;
mod stt;
mod system;
mod tools;
mod tts;
mod undo;
mod voice;
mod weather;

use std::sync::Arc;

use tauri::menu::{CheckMenuItem, CheckMenuItemBuilder, MenuBuilder, MenuItem, MenuItemBuilder};
use tauri::tray::TrayIconBuilder;
use tauri::{ActivationPolicy, App, AppHandle, Manager, RunEvent, Wry};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

/// Skrót „Powiedz coś”
const TALK_SHORTCUT: (Option<Modifiers>, Code) = (Some(Modifiers::SUPER.union(Modifiers::SHIFT)), Code::KeyK);
/// Skrót przełączający nasłuch „Słuchaj”
const LISTEN_SHORTCUT: (Option<Modifiers>, Code) = (Some(Modifiers::SUPER.union(Modifiers::SHIFT)), Code::KeyL);
const TALK_LABEL: &str = "⌘⇧K";
const LISTEN_LABEL: &str = "⌘⇧L";

/// Uchwyty do elementów sterowania, żeby menu i skróty robiły to samo
struct Controls {
    voice: Arc<voice::VoiceState>,
    listen: CheckMenuItem<Wry>,
    last: MenuItem<Wry>,
}

fn shortcut(parts: (Option<Modifiers>, Code)) -> Shortcut {
    Shortcut::new(parts.0, parts.1)
}

fn talk_now(app: &AppHandle, c: &Controls) {
    voice::spawn_turn(c.voice.clone(), busy_title(app.clone(), c.listen.clone()), last_text(c.last.clone()));
}

fn set_listen(app: &AppHandle, c: &Controls, on: bool) {
    let _ = c.listen.set_checked(on);
    voice::set_listening(
        c.voice.clone(),
        on,
        Arc::new(busy_title(app.clone(), c.listen.clone())),
        Arc::new(last_text(c.last.clone())),
    );
    set_tray_title(app, if on { Some("●") } else { None });
}

fn setup(app: &mut App) -> Result<(), Box<dyn std::error::Error>> {
    // Bez ikony w Docku: aplikacja żyje tylko w pasku menu
    #[cfg(target_os = "macos")]
    app.set_activation_policy(ActivationPolicy::Accessory);

    let talk = MenuItemBuilder::with_id("talk", "Powiedz coś").accelerator(TALK_LABEL).build(app)?;
    let last: MenuItem<Wry> = MenuItemBuilder::with_id("last", "Ostatnio: —").enabled(false).build(app)?;
    let listen = CheckMenuItemBuilder::with_id("listen", "Słuchaj").checked(false).accelerator(LISTEN_LABEL).build(app)?;
    tts::load_mute();
    let mute = CheckMenuItemBuilder::with_id("mute", "Wycisz głos").checked(tts::is_muted()).build(app)?;
    let quit = MenuItemBuilder::with_id("quit", "Wyjdź z Klaudiusza").build(app)?;
    let menu = MenuBuilder::new(app)
        .item(&talk)
        .item(&last)
        .separator()
        .item(&listen)
        .item(&mute)
        .separator()
        .item(&quit)
        .build()?;

    // przypomnienia działają w tle, niezależnie od przycisku „Słuchaj”
    notice::request_permission();
    alerts::start_loop();
    // model ładujemy z góry w tle, żeby pierwsza odpowiedź nie czekała
    std::thread::spawn(llm::warm_up);

    app.manage(Controls { voice: voice::VoiceState::new(), listen: listen.clone(), last: last.clone() });
    app.global_shortcut().register(shortcut(TALK_SHORTCUT))?;
    app.global_shortcut().register(shortcut(LISTEN_SHORTCUT))?;

    TrayIconBuilder::with_id("klaudiusz")
        .icon(tauri::include_image!("./icons/tray.png"))
        // ikona szablonowa: macOS sam dopasuje kolor do jasnego/ciemnego paska menu
        .icon_as_template(true)
        .tooltip("Klaudiusz")
        .menu(&menu)
        .on_menu_event(move |app, event| {
            let mute_item = mute.clone();
            let Some(c) = app.try_state::<Controls>() else { return };
            match event.id().as_ref() {
                "talk" => talk_now(app, &c),
                "listen" => {
                    let on = c.listen.is_checked().unwrap_or(false);
                    set_listen(app, &c, on);
                }
                "mute" => {
                    let on = mute_item.is_checked().unwrap_or(false);
                    tts::set_muted(on);
                }
                "quit" => app.exit(0),
                _ => {}
            }
        })
        .build(app)?;

    Ok(())
}

/// Pokazuje ostatnią linijkę rozmowy w menu (skrócona)
fn last_text(item: MenuItem<Wry>) -> impl Fn(&str) + Send + Sync + 'static {
    move |text: &str| {
        let short: String = text.chars().take(90).collect();
        let _ = item.set_text(short);
    }
}

fn set_tray_title(app: &AppHandle, title: Option<&str>) {
    if let Some(tray) = app.tray_by_id("klaudiusz") {
        let _ = tray.set_title(title);
    }
}

/// Pasek menu pokazuje „…” podczas tury, a po niej wraca do stanu nasłuchu
fn busy_title(app: AppHandle, listen: CheckMenuItem<Wry>) -> impl Fn(bool) + Send + Sync + 'static {
    move |busy| {
        let on = listen.is_checked().unwrap_or(false);
        let title = if busy {
            Some("…")
        } else if on {
            Some("●")
        } else {
            None
        };
        set_tray_title(&app, title);
    }
}

pub fn run() {
    tauri::Builder::default()
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, pressed, event| {
                    if event.state() != ShortcutState::Pressed {
                        return;
                    }
                    let Some(c) = app.try_state::<Controls>() else { return };
                    if *pressed == shortcut(TALK_SHORTCUT) {
                        talk_now(app, &c);
                    } else if *pressed == shortcut(LISTEN_SHORTCUT) {
                        let on = !c.listen.is_checked().unwrap_or(false);
                        set_listen(app, &c, on);
                    }
                })
                .build(),
        )
        .setup(setup)
        .build(tauri::generate_context!())
        .expect("nie udało się zbudować aplikacji Klaudiusz")
        .run(|_app, event| {
            // Brak okien to nie powód do zamknięcia: żyjemy w pasku menu
            if let RunEvent::ExitRequested { api, code, .. } = event {
                if code.is_none() {
                    api.prevent_exit();
                }
            }
        });
}
