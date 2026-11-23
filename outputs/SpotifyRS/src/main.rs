#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

use slint::{ComponentHandle, Image, Model, ModelRc, Rgba8Pixel, SharedPixelBuffer, VecModel};
use spotifyrs::{
    library::{self, Library, PAGE_SIZE},
    model,
    settings::{self, Settings},
    soloist_launcher,
    worker::{self, Command, Snapshot},
};
use std::{
    rc::Rc,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{sync_channel, SyncSender},
        Arc,
    },
};
slint::include_modules!();

mod track_art;

fn dispatch(ui: &slint::Weak<AppWindow>, tx: &SyncSender<Command>, command: Command) {
    if let Some(ui) = ui.upgrade() {
        match tx.try_send(command) {
            Ok(()) => ui.set_busy(true),
            Err(_) => ui.set_status("Aguarda a conclusão do pedido anterior.".into()),
        }
    }
}

fn render(ui: &AppWindow, state: Snapshot, library_changed: bool, playlists_changed: bool) {
    if (ui.get_page() == 2
        && ((state.connected && !ui.get_connected())
            || (state.library.ready && !ui.get_library_ready())))
        || (state.demo != ui.get_demo())
    {
        ui.set_page(0);
    }
    ui.set_soloist(state.soloist);
    ui.set_connected(state.connected);
    ui.set_demo(state.demo);
    ui.set_status(state.status.into());
    ui.set_fresh(state.fresh);
    ui.set_authorizing(state.authorizing);
    ui.set_catalog_ready(state.catalog_ready);
    if library_changed {
        render_library(ui, &state.library, playlists_changed);
    }
    ui.set_search_note(state.search_note.into());
    ui.set_queue_note(state.queue_note.into());
    if let Some(tracks) = changed_tracks(&ui.get_tracks(), &state.results) {
        ui.set_tracks(tracks);
    }
    if let Some(queue) = changed_tracks(&ui.get_queue(), &state.queue) {
        ui.set_queue(queue);
    }
    let p = &state.playback;
    ui.set_playing(p.is_playing);
    ui.set_can_toggle(state.fresh && p.permits(if p.is_playing { "pausing" } else { "resuming" }));
    ui.set_can_next(state.fresh && p.permits("skipping_next"));
    ui.set_can_previous(state.fresh && p.permits("skipping_prev"));
    ui.set_volume(
        p.device
            .as_ref()
            .and_then(model::Device::adjustable_volume)
            .map(i32::from)
            .unwrap_or(-1),
    );
    ui.set_volume_device(
        p.device
            .as_ref()
            .and_then(|device| device.id.as_deref())
            .unwrap_or_default()
            .into(),
    );
    let title = p
        .item
        .as_ref()
        .map(|i| i.name.as_str())
        .unwrap_or("À espera da tua música");
    ui.set_track_title(title.into());
    ui.set_track_uri(
        p.item
            .as_ref()
            .map(|item| item.uri.as_str())
            .unwrap_or_default()
            .into(),
    );
    ui.set_artist(
        p.item
            .as_ref()
            .map(model::Item::artist_line)
            .unwrap_or_else(|| "Escolhe o que queres ouvir".into())
            .into(),
    );
    ui.set_device_name(
        p.device
            .as_ref()
            .map(|d| d.name.as_str())
            .unwrap_or("Nenhum dispositivo ativo")
            .into(),
    );
    let duration = p.item.as_ref().map(|i| i.duration_ms).unwrap_or(0);
    let elapsed = p.progress_ms.unwrap_or(0).min(duration);
    ui.set_elapsed(model::clock(elapsed).into());
    ui.set_duration(model::clock(duration).into());
    ui.set_elapsed_ms(elapsed.min(i32::MAX as u64) as i32);
    ui.set_duration_ms(duration.min(i32::MAX as u64) as i32);
    let spotify_url = p
        .item
        .as_ref()
        .and_then(|i| i.external_urls.get("spotify"))
        .filter(|s| {
            url::Url::parse(s)
                .is_ok_and(|u| u.scheme() == "https" && u.host_str() == Some("open.spotify.com"))
        });
    ui.set_spotify_url(
        spotify_url
            .map(String::as_str)
            .unwrap_or("https://open.spotify.com")
            .into(),
    );
    ui.set_progress(if duration > 0 {
        elapsed as f32 / duration as f32
    } else {
        0.0
    });
    let active = state.devices.iter().position(|d| {
        p.device
            .as_ref()
            .and_then(|p| p.id.as_ref())
            .is_some_and(|id| d.id.as_ref() == Some(id))
    });
    let names: Vec<slint::SharedString> = state
        .devices
        .iter()
        .map(|d| {
            format!(
                "{}{}",
                d.name,
                if d.is_restricted { " (restrito)" } else { "" }
            )
            .into()
        })
        .collect();
    let existing = ui.get_devices();
    let changed = existing.row_count() != names.len()
        || names
            .iter()
            .enumerate()
            .any(|(i, name)| existing.row_data(i).as_ref() != Some(name));
    if changed {
        ui.set_devices(ModelRc::from(Rc::new(VecModel::from(names))));
        ui.set_selected_device(active.map(|i| i as i32).unwrap_or(-1));
    }
    let art_url = p
        .item
        .as_ref()
        .map_or_else(slint::SharedString::default, |item| {
            if item.artwork_url().is_empty() && !item.uri.is_empty() {
                track_art::known_url(ui, &item.uri)
            } else {
                item.artwork_url().into()
            }
        });
    ui.set_track_art(track_art::image(art_url.as_str()));
    ui.set_track_art_url(art_url);
    ui.set_track_demo_tone(p.item.as_ref().map_or(-1, |item| demo_tone(&item.uri)));
    ui.set_busy(state.busy);
}

fn playlist_rows(library: &Library) -> Vec<PlaylistRow> {
    library
        .playlists
        .iter()
        .enumerate()
        .map(|(index, playlist)| PlaylistRow {
            id: playlist.id.as_str().into(),
            title: playlist.name.as_str().into(),
            art: playlist
                .cover
                .as_ref()
                .map(|cover| {
                    Image::from_rgba8(SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
                        &cover.pixels,
                        cover.width,
                        cover.height,
                    ))
                })
                .unwrap_or_default(),
            tone: (index % 4) as i32,
        })
        .collect()
}

fn render_library(ui: &AppWindow, library: &Library, playlists_changed: bool) {
    ui.set_library_ready(library.ready);
    ui.set_library_note(library.note.as_str().into());
    ui.set_playlist_offset(library.playlist_offset as i32);
    ui.set_more_playlists(library.more_playlists);
    ui.set_collection_id(library.selection.as_str().into());
    ui.set_collection_note(library.track_note.as_str().into());
    ui.set_track_offset(library.track_offset as i32);
    ui.set_more_collection_tracks(library.more_tracks);
    if playlists_changed {
        let rows = playlist_rows(library);
        ui.set_featured(ModelRc::from(Rc::new(VecModel::from(
            rows.iter().take(4).cloned().collect::<Vec<_>>(),
        ))));
        ui.set_playlists(ModelRc::from(Rc::new(VecModel::from(rows))));
    }
    if let Some(tracks) = changed_tracks(&ui.get_saved_preview(), &library.saved_preview) {
        ui.set_saved_preview(tracks);
    }
    if let Some(tracks) = changed_tracks(&ui.get_collection_tracks(), &library.tracks) {
        ui.set_collection_tracks(tracks);
    }
}

fn demo_tone(uri: &str) -> i32 {
    uri.strip_prefix("demo:")
        .and_then(|index| index.parse::<usize>().ok())
        .map_or(-1, |index| (index % 3) as i32)
}

fn changed_tracks(
    existing: &ModelRc<TrackRow>,
    items: &[model::Item],
) -> Option<ModelRc<TrackRow>> {
    let rows: Vec<_> = items
        .iter()
        .map(|item| TrackRow {
            art: track_art::image(item.artwork_url()),
            art_url: item.artwork_url().into(),
            demo_tone: demo_tone(&item.uri),
            uri: item.uri.as_str().into(),
            title: item.name.as_str().into(),
            artist: item.artist_line().into(),
            length: if item.duration_ms == 0 {
                "—".into()
            } else {
                model::clock(item.duration_ms).into()
            },
        })
        .collect();
    if existing.row_count() == rows.len()
        && rows
            .iter()
            .enumerate()
            .all(|(i, row)| existing.row_data(i).as_ref() == Some(row))
    {
        return None;
    }
    Some(ModelRc::from(Rc::new(VecModel::from(rows))))
}

fn current_settings(ui: &AppWindow) -> Settings {
    let mut settings = Settings {
        soloist_address: ui.get_soloist_address().into(),
        soloist_executable: ui.get_soloist_executable().into(),
        client_id: ui.get_client_id().into(),
        remember_soloist_key: ui.get_remember_soloist_key(),
        auto_start_soloist: ui.get_auto_start_soloist(),
    };
    settings.normalize();
    settings
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let ui = AppWindow::new()?;
    ui.set_app_version(
        option_env!("SPOTIFYRS_BUILD_VERSION")
            .unwrap_or(env!("CARGO_PKG_VERSION"))
            .into(),
    );
    track_art::start(&ui)?;
    ui.set_demo_preview(ModelRc::from(Rc::new(VecModel::from(playlist_rows(
        &library::demo(),
    )))));
    let demo_tracks: Vec<_> = (0..model::DEMO_TRACKS.len())
        .map(model::demo_track)
        .collect();
    if let Some(rows) = changed_tracks(&ModelRc::default(), &demo_tracks) {
        ui.set_demo_songs(rows);
    }
    #[cfg(target_os = "linux")]
    slint::set_xdg_app_id("spotifyrs")?;
    let settings_path = settings::path();
    let mut saved = settings_path
        .as_deref()
        .map(Settings::load)
        .unwrap_or_default();
    saved.normalize();
    let soloist_launch_supported = cfg!(any(windows, target_os = "linux"));
    ui.set_soloist_launch_supported(soloist_launch_supported);
    ui.set_soloist_address(saved.soloist_address.as_str().into());
    ui.set_remember_soloist_key(saved.remember_soloist_key);
    ui.set_auto_start_soloist(saved.auto_start_soloist);
    if saved.soloist_executable.is_empty() {
        if let Some(path) = soloist_launcher::find_executable() {
            ui.set_soloist_executable(path.to_string_lossy().as_ref().into());
        }
    } else {
        ui.set_soloist_executable(saved.soloist_executable.as_str().into());
    }
    if !saved.client_id.is_empty() {
        ui.set_client_id(saved.client_id.as_str().into());
    } else if let Ok(id) = std::env::var("SPOTIFY_CLIENT_ID") {
        ui.set_client_id(id.into());
    }
    let initial = current_settings(&ui);
    let (tx, rx) = sync_channel::<Command>(4);
    let cancelled = Arc::new(AtomicBool::new(false));
    let worker_ui = ui.as_weak();
    let worker_cancel = cancelled.clone();
    let handle = std::thread::Builder::new()
        .name("spotify-api".into())
        .spawn(move || {
            let mut previous_library = Arc::new(Library::default());
            worker::run(rx, worker_cancel, move |state| {
                let library_changed = !Arc::ptr_eq(&previous_library, &state.library);
                let playlists_changed =
                    !Arc::ptr_eq(&previous_library.playlists, &state.library.playlists);
                previous_library = state.library.clone();
                let _ = worker_ui.upgrade_in_event_loop(move |ui| {
                    render(&ui, state, library_changed, playlists_changed)
                });
            });
        })?;
    {
        let (weak, tx, cancel) = (ui.as_weak(), tx.clone(), cancelled.clone());
        ui.on_login(move |id| {
            cancel.store(false, Ordering::Relaxed);
            dispatch(&weak, &tx, Command::Login(id.to_string()));
        });
    }
    {
        let (weak, tx, cancel) = (ui.as_weak(), tx.clone(), cancelled.clone());
        ui.on_authorize_catalog(move |id| {
            cancel.store(false, Ordering::Relaxed);
            dispatch(&weak, &tx, Command::Catalog(id.to_string()));
        });
    }
    {
        let cancel = cancelled.clone();
        ui.on_cancel_auth(move || cancel.store(true, Ordering::Relaxed));
    }
    {
        let (weak, tx, cancel) = (ui.as_weak(), tx.clone(), cancelled.clone());
        ui.on_authorize_library(move |id| {
            cancel.store(false, Ordering::Relaxed);
            dispatch(&weak, &tx, Command::AuthorizeLibrary(id.to_string()));
        });
    }
    {
        let (weak, tx) = (ui.as_weak(), tx.clone());
        ui.on_load_library(move |page| {
            dispatch(
                &weak,
                &tx,
                Command::LoadLibrary(page.max(0) as usize * PAGE_SIZE),
            );
        });
    }
    {
        let (weak, tx) = (ui.as_weak(), tx.clone());
        ui.on_open_collection(move |id, page| {
            dispatch(
                &weak,
                &tx,
                Command::OpenCollection(id.to_string(), page.max(0) as usize * PAGE_SIZE),
            );
        });
    }
    {
        let weak = ui.as_weak();
        ui.on_open_content(move |kind, id| {
            let destination = match (kind.as_str(), id.as_str()) {
                ("collection", library::SAVED) => {
                    "https://open.spotify.com/collection/tracks".to_owned()
                }
                ("collection", id) if library::valid_id(id) => {
                    format!("https://open.spotify.com/playlist/{id}")
                }
                ("track", uri) => {
                    let Some(uri) = model::track_uri(uri) else {
                        return;
                    };
                    format!(
                        "https://open.spotify.com/track/{}",
                        &uri["spotify:track:".len()..]
                    )
                }
                _ => return,
            };
            if webbrowser::open(&destination).is_err() {
                if let Some(ui) = weak.upgrade() {
                    ui.set_status("Não foi possível abrir o navegador.".into());
                }
            }
        });
    }
    {
        let (weak, tx, cancel) = (ui.as_weak(), tx.clone(), cancelled.clone());
        ui.on_logout(move || {
            cancel.store(true, Ordering::Relaxed);
            dispatch(&weak, &tx, Command::Logout);
        });
    }
    macro_rules! bind {
        ($callback:ident, $command:expr) => {{
            let (weak, tx) = (ui.as_weak(), tx.clone());
            ui.$callback(move || dispatch(&weak, &tx, $command));
        }};
    }
    {
        let (weak, tx) = (ui.as_weak(), tx.clone());
        ui.on_soloist_connect(move |address| {
            dispatch(&weak, &tx, Command::Soloist(address.to_string()))
        });
    }
    {
        let (weak, tx, cancel) = (ui.as_weak(), tx.clone(), cancelled.clone());
        ui.on_soloist_start(move |address, executable, key, remember| {
            let Some(ui) = weak.upgrade() else { return };
            cancel.store(false, Ordering::Relaxed);
            let command = Command::StartSoloist {
                address: address.to_string(),
                executable: executable.to_string(),
                key: key.to_string(),
                remember,
            };
            if tx.try_send(command).is_ok() {
                ui.set_soloist_key("".into());
                ui.set_busy(true);
            } else {
                ui.set_status("Aguarda a conclusão do pedido anterior.".into());
            }
        });
    }
    {
        let (weak, tx) = (ui.as_weak(), tx.clone());
        ui.on_forget_soloist_key(move || {
            if let Some(ui) = weak.upgrade() {
                ui.set_remember_soloist_key(false);
                ui.set_soloist_key("".into());
            }
            dispatch(&weak, &tx, Command::ForgetSoloistKey);
        });
    }
    bind!(on_demo_start, Command::Demo);
    bind!(on_forget_authorization, Command::ForgetAuthorization);
    bind!(on_load_queue, Command::LoadQueue);
    bind!(on_refresh, Command::Refresh);
    bind!(on_toggle, Command::Toggle);
    bind!(on_next, Command::Next);
    bind!(on_previous, Command::Previous);
    {
        let (weak, tx) = (ui.as_weak(), tx.clone());
        ui.on_set_volume(move |volume| {
            dispatch(&weak, &tx, Command::SetVolume(volume.clamp(0, 100) as u8));
        });
    }
    {
        let (weak, tx) = (ui.as_weak(), tx.clone());
        ui.on_search(move |query| dispatch(&weak, &tx, Command::Search(query.to_string())));
    }
    {
        let (weak, tx) = (ui.as_weak(), tx.clone());
        ui.on_play_track(move |uri| dispatch(&weak, &tx, Command::PlayTrack(uri.to_string())));
    }
    {
        let (weak, tx) = (ui.as_weak(), tx.clone());
        ui.on_transfer(move |index| {
            if index >= 0 {
                dispatch(&weak, &tx, Command::Transfer(index as usize));
            }
        });
    }
    {
        let weak = ui.as_weak();
        ui.on_open_spotify(move || {
            if let Some(ui) = weak.upgrade() {
                if webbrowser::open(ui.get_spotify_url().as_str()).is_err() {
                    ui.set_status("Não foi possível abrir o navegador.".into());
                }
            }
        });
    }
    {
        let weak = ui.as_weak();
        ui.on_progress_tick(move || {
            if let Some(ui) = weak.upgrade() {
                let elapsed = ui
                    .get_elapsed_ms()
                    .saturating_add(1000)
                    .min(ui.get_duration_ms());
                ui.set_elapsed_ms(elapsed);
                ui.set_elapsed(model::clock(elapsed as u64).into());
                if ui.get_duration_ms() > 0 {
                    ui.set_progress(elapsed as f32 / ui.get_duration_ms() as f32);
                }
            }
        });
    }
    {
        let weak = ui.as_weak();
        ui.on_open_dashboard(move || {
            if webbrowser::open("https://developer.spotify.com/dashboard").is_err() {
                if let Some(ui) = weak.upgrade() {
                    ui.set_status("Não foi possível abrir o navegador.".into());
                }
            }
        });
    }
    if std::env::args().any(|arg| arg == "--demo") {
        let _ = tx.try_send(Command::Demo);
    } else {
        if !initial.client_id.is_empty() {
            let _ = tx.try_send(Command::RestoreAuthorization(initial.client_id.clone()));
        }
        if soloist_launch_supported && initial.auto_start_soloist {
            dispatch(
                &ui.as_weak(),
                &tx,
                Command::StartSoloist {
                    address: initial.soloist_address.clone(),
                    executable: initial.soloist_executable.clone(),
                    key: String::new(),
                    remember: false,
                },
            );
        }
        if !initial.client_id.is_empty() {
            let _ = tx.try_send(Command::LoadLibrary(0));
        }
    }
    let result = ui.run();
    let saved = if let Some(path) = settings_path {
        let current = current_settings(&ui);
        if current != initial {
            current.save(&path)
        } else {
            Ok(())
        }
    } else {
        Ok(())
    };
    cancelled.store(true, Ordering::Relaxed);
    let _ = tx.send(Command::Stop);
    let _ = handle.join();
    saved?;
    result.map_err(Into::into)
}
