use crate::{
    api::{self, ApiError, SpotifyApi},
    auth,
    library::{self, Library},
    model::{self, Device, Item, Playback, DEMO_TRACKS},
};
use rand::seq::SliceRandom;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver, RecvTimeoutError},
        Arc,
    },
    time::{Duration, Instant},
};

pub enum Command {
    Login(String),
    Soloist {
        address: String,
        activate: bool,
    },
    StartSoloist {
        address: String,
        executable: String,
        key: String,
        remember: bool,
        activate: bool,
    },
    ForgetSoloistKey,
    ForgetAuthorization,
    RestoreAuthorization(String),
    Catalog(String),
    AuthorizeLibrary(String),
    LoadLibrary(usize),
    OpenCollection(String, usize),
    Search(String),
    LoadQueue,
    PlayTrack(String),
    Demo,
    Logout,
    Refresh,
    Poll,
    Toggle,
    Next,
    Previous,
    SetShuffle(bool),
    SetVolume(u8),
    Seek {
        uri: String,
        device_id: String,
        position_ms: u64,
    },
    Transfer(usize),
    Stop,
}

#[derive(Clone, Default)]
pub struct Snapshot {
    pub soloist: bool,
    pub connected: bool,
    pub demo: bool,
    pub busy: bool,
    pub fresh: bool,
    pub authorizing: bool,
    pub catalog_ready: bool,
    pub library: Arc<Library>,
    pub search_note: String,
    pub queue_note: String,
    pub results: Arc<[Item]>,
    pub queue: Arc<[Item]>,
    pub status: String,
    pub playback: Playback,
    pub playback_updated_at: Option<Instant>,
    pub devices: Vec<Device>,
}

struct DemoQueue {
    order: [usize; DEMO_TRACKS.len()],
    cursor: usize,
}

impl DemoQueue {
    fn new(current: usize, shuffled: bool) -> Self {
        let mut order = std::array::from_fn(|index| (current + index) % DEMO_TRACKS.len());
        if shuffled {
            order[1..].shuffle(&mut rand::thread_rng());
        }
        Self { order, cursor: 0 }
    }

    fn current(&self) -> usize {
        self.order[self.cursor]
    }

    fn step(&mut self, next: bool) {
        let len = self.order.len();
        self.cursor = (self.cursor + if next { 1 } else { len - 1 }) % len;
    }

    fn select(&mut self, index: usize) {
        if let Some(cursor) = self.order.iter().position(|track| *track == index) {
            self.cursor = cursor;
        }
    }

    fn upcoming(&self) -> impl Iterator<Item = Item> + '_ {
        (1..self.order.len())
            .map(|offset| model::demo_track(self.order[(self.cursor + offset) % self.order.len()]))
    }
}

fn load_library(
    api: &mut SpotifyApi,
    client: &reqwest::blocking::Client,
    library: &mut Library,
    offset: usize,
) {
    let (mut playlists, more) = match api.playlists(offset) {
        Ok(page) => page,
        Err(error) => {
            library.note = error.to_string();
            return;
        }
    };
    for playlist in playlists.iter_mut().take(4) {
        let image = playlist
            .images
            .iter()
            .flatten()
            .min_by_key(|image| image.width.unwrap_or(300).abs_diff(200));
        playlist.cover = image
            .and_then(|image| SpotifyApi::cover(client, &image.url))
            .map(Arc::new);
    }
    library.note = if playlists.is_empty() {
        "Sem playlists nesta página.".into()
    } else {
        String::new()
    };
    library.playlists = playlists.into();
    library.playlist_offset = offset;
    library.more_playlists = more;
    if offset == 0 {
        if let Ok((tracks, _)) = api.library_tracks(library::SAVED, 0) {
            library.saved_preview = tracks.into_iter().take(5).collect();
        }
    }
}

pub fn run(
    receiver: Receiver<Command>,
    cancelled: Arc<AtomicBool>,
    mut emit: impl FnMut(Snapshot),
) {
    let client = match api::http_client() {
        Ok(client) => client,
        Err(_) => {
            emit(Snapshot {
                status: "Não foi possível iniciar a ligação segura ao Spotify.".into(),
                ..Default::default()
            });
            return;
        }
    };
    let mut soloist = None;
    let mut activate_when_paired = false;
    let mut managed_soloist = None;
    let mut api: Option<SpotifyApi> = None;
    let mut catalog: Option<SpotifyApi> = None;
    let mut library_api = None;
    let mut state = Snapshot::default();
    let mut demo_queue = DemoQueue::new(0, false);
    let mut next_poll = Instant::now() + Duration::from_secs(60);
    let mut suspended = false;
    let mut failures = 0u32;
    let mut demo_updated_at = Instant::now();
    loop {
        let polling = (soloist.is_some() || (api.is_some() && !state.soloist)) && !suspended;
        let demo_remaining = state
            .playback
            .item
            .as_ref()
            .filter(|_| state.demo && state.playback.is_playing)
            .map(|item| {
                Duration::from_millis(
                    item.duration_ms
                        .saturating_sub(state.playback.progress_ms.unwrap_or(0)),
                )
                .saturating_sub(demo_updated_at.elapsed())
            });
        let wait = if let Some(remaining) = demo_remaining {
            remaining
        } else if polling {
            next_poll.saturating_duration_since(Instant::now())
        } else {
            Duration::from_secs(3600)
        };
        let mut command = match receiver.recv_timeout(wait) {
            Ok(Command::Stop) | Err(RecvTimeoutError::Disconnected) => return,
            Ok(command) => command,
            Err(RecvTimeoutError::Timeout) if polling || demo_remaining.is_some() => Command::Poll,
            Err(RecvTimeoutError::Timeout) => continue,
        };
        let mut launch_note = None;
        if let Command::StartSoloist {
            address,
            executable,
            key,
            remember,
            activate,
        } = command
        {
            state.busy = true;
            state.status = "A iniciar o Soloist…".into();
            emit(state.clone());
            let result = crate::soloist::endpoint(&address).and_then(|address| {
                crate::soloist_launcher::start(address, &executable, &key, remember, &cancelled)
            });
            match result {
                Ok((child, warning)) => {
                    if let Some(child) = child {
                        managed_soloist = Some(child);
                    }
                    launch_note = warning;
                    command = Command::Soloist { address, activate };
                }
                Err(error) => {
                    state.busy = false;
                    state.status = error;
                    emit(state.clone());
                    continue;
                }
            }
        }
        if matches!(command, Command::ForgetSoloistKey) {
            state.busy = false;
            state.status = match crate::soloist_launcher::forget_key() {
                Ok(()) => "A chave guardada do Soloist foi removida.".into(),
                Err(_) => "Não foi possível remover a chave guardada.".into(),
            };
            emit(state.clone());
            continue;
        }
        if matches!(command, Command::ForgetAuthorization) {
            let catalog_deleted = auth::forget_session("").is_ok();
            let web_deleted = auth::forget_session(auth::SCOPES).is_ok();
            let library_deleted = auth::forget_session(auth::LIBRARY_SCOPES).is_ok();
            library_api = None;
            catalog = None;
            if api.take().is_some() && !state.soloist {
                state.connected = false;
                state.fresh = false;
                state.playback = Playback::default();
                state.queue = Arc::default();
                state.queue_note.clear();
                state.devices.clear();
            }
            if !state.demo {
                state.library = Arc::default();
                state.results = Arc::default();
                state.search_note.clear();
            }
            state.catalog_ready = false;
            state.busy = false;
            state.status = if catalog_deleted && web_deleted && library_deleted {
                "A autorização guardada foi removida deste PC."
            } else {
                "Não foi possível remover toda a autorização do cofre do sistema."
            }
            .into();
            emit(state.clone());
            continue;
        }
        if state.demo {
            if state.playback.is_playing {
                let duration = state
                    .playback
                    .item
                    .as_ref()
                    .map(|item| item.duration_ms)
                    .unwrap_or(0);
                state.playback.progress_ms = Some(
                    state
                        .playback
                        .progress_ms
                        .unwrap_or(0)
                        .saturating_add(demo_updated_at.elapsed().as_millis() as u64)
                        .min(duration),
                );
            }
            demo_updated_at = Instant::now();
            state.playback_updated_at = Some(demo_updated_at);
            if state.playback.is_playing
                && state
                    .playback
                    .item
                    .as_ref()
                    .is_some_and(|item| state.playback.progress_ms == Some(item.duration_ms))
            {
                demo_queue.step(true);
                state.playback.item = Some(model::demo_track(demo_queue.current()));
                state.playback.progress_ms = Some(0);
                state.queue = demo_queue.upcoming().collect();
            }
        }
        match &command {
            Command::RestoreAuthorization(id) => {
                library_api = SpotifyApi::restore(client.clone(), id, auth::LIBRARY_SCOPES);
                Arc::make_mut(&mut state.library).ready = library_api.is_some();
                catalog = SpotifyApi::restore(client.clone(), id, "")
                    .or_else(|| SpotifyApi::restore(client.clone(), id, auth::SCOPES));
                state.catalog_ready = catalog.is_some() || library_api.is_some();
                if state.catalog_ready {
                    state.status = "Autorização da pesquisa recuperada.".into();
                    emit(state.clone());
                }
                continue;
            }
            Command::AuthorizeLibrary(id) => {
                state.busy = true;
                state.authorizing = true;
                state.status = "Autoriza a biblioteca no navegador.".into();
                emit(state.clone());
                match auth::login(&client, id.trim(), &cancelled, auth::LIBRARY_SCOPES) {
                    Ok(session) if !cancelled.load(Ordering::Relaxed) => {
                        let mut library =
                            SpotifyApi::new(client.clone(), id.trim().into(), session);
                        let remembered = library.remember().is_ok();
                        state.authorizing = false;
                        state.status = "A carregar a biblioteca…".into();
                        emit(state.clone());
                        state.status = if remembered {
                            "Biblioteca autorizada."
                        } else {
                            "Biblioteca autorizada. Não foi possível guardá-la no cofre do sistema."
                        }
                        .into();
                        let mut data = Library {
                            ready: true,
                            ..Library::default()
                        };
                        load_library(&mut library, &client, &mut data, 0);
                        library_api = Some(library);
                        state.library = Arc::new(data);
                        if state.demo {
                            state.demo = false;
                            state.fresh = false;
                            state.playback = Playback::default();
                            state.devices.clear();
                            state.results = Arc::default();
                            state.queue = Arc::default();
                            state.search_note.clear();
                            state.queue_note.clear();
                        }
                    }
                    Ok(_) => state.status = "Autorização cancelada.".into(),
                    Err(error) => state.status = error.to_string(),
                }
                state.catalog_ready = catalog.is_some() || api.is_some() || library_api.is_some();
                state.authorizing = false;
                state.busy = false;
                emit(state.clone());
                continue;
            }
            Command::LoadLibrary(offset) => {
                if let Some(library_api) = library_api.as_mut().filter(|_| !state.demo) {
                    state.busy = true;
                    emit(state.clone());
                    load_library(
                        library_api,
                        &client,
                        Arc::make_mut(&mut state.library),
                        *offset,
                    );
                }
                state.busy = false;
                emit(state.clone());
                continue;
            }
            Command::OpenCollection(id, offset) => {
                let result = if state.demo {
                    Ok((library::demo_tracks(id), false))
                } else if let Some(api) = library_api.as_mut() {
                    api.library_tracks(id, *offset).map_err(|error| {
                        if matches!(error, ApiError::Forbidden) && id != library::SAVED {
                            "O Spotify não dá acesso às faixas desta playlist. Podes abri-la no Spotify.".into()
                        } else { error.to_string() }
                    })
                } else {
                    Err("Autoriza a biblioteca em Ligações.".into())
                };
                let library = Arc::make_mut(&mut state.library);
                match result {
                    Ok((tracks, more)) => {
                        library.selection.clone_from(id);
                        library.track_offset = *offset;
                        library.track_note = if tracks.is_empty() {
                            "Sem faixas disponíveis nesta página.".into()
                        } else {
                            String::new()
                        };
                        library.tracks = tracks.into();
                        library.more_tracks = more;
                    }
                    Err(error) => {
                        if library.selection != *id {
                            library.selection.clone_from(id);
                            library.tracks = Arc::default();
                            library.track_offset = 0;
                            library.more_tracks = false;
                        }
                        library.track_note = error;
                    }
                }
                state.busy = false;
                emit(state.clone());
                continue;
            }
            Command::Catalog(id) => {
                state.busy = true;
                state.authorizing = true;
                state.status = "Autoriza o catálogo no navegador.".into();
                emit(state.clone());
                match auth::login(&client, id.trim(), &cancelled, "") {
                    Ok(session) if !cancelled.load(Ordering::Relaxed) => {
                        let mut search = SpotifyApi::new(client.clone(), id.trim().into(), session);
                        state.status = if search.remember().is_ok() {
                            "Catálogo autorizado. Já podes pesquisar faixas."
                        } else {
                            "Catálogo autorizado. Não foi possível guardá-lo no cofre do sistema."
                        }
                        .into();
                        catalog = Some(search);
                        state.search_note.clear();
                    }
                    Ok(_) => state.status = "Autorização cancelada.".into(),
                    Err(error) => state.status = error.to_string(),
                }
                state.catalog_ready = catalog.is_some() || api.is_some() || library_api.is_some();
                state.authorizing = false;
                state.busy = false;
                emit(state.clone());
                continue;
            }
            Command::Search(query) => {
                let query = query.trim();
                let result = if query.is_empty() || query.chars().count() > 160 {
                    Err("Escreve um nome ou artista com até 160 caracteres.".into())
                } else if state.demo {
                    let query = query.to_lowercase();
                    Ok((0..DEMO_TRACKS.len())
                        .map(model::demo_track)
                        .filter(|item| {
                            item.name.to_lowercase().contains(&query)
                                || item.artist_line().to_lowercase().contains(&query)
                        })
                        .collect::<Vec<_>>())
                } else if let Some(search_api) =
                    catalog.as_mut().or(api.as_mut()).or(library_api.as_mut())
                {
                    match search_api.search_tracks(query) {
                        Ok(items) => Ok(items),
                        Err(error) => {
                            if matches!(
                                error,
                                ApiError::Unauthorized | ApiError::Auth(auth::AuthError::Expired)
                            ) && catalog.is_some()
                            {
                                catalog = None;
                                state.catalog_ready = api.is_some() || library_api.is_some();
                            }
                            Err(error.to_string())
                        }
                    }
                } else {
                    Err("Autoriza o catálogo em Ligações para pesquisar por nome.".into())
                };
                state.results = Arc::default();
                state.search_note = match result {
                    Ok(items) => {
                        let count = items.len();
                        state.results = items.into();
                        match count {
                            0 => "Não foram encontradas faixas.".into(),
                            1 => "1 faixa encontrada".into(),
                            _ => format!("{count} faixas encontradas"),
                        }
                    }
                    Err(error) => error,
                };
                state.busy = false;
                emit(state.clone());
                continue;
            }
            Command::LoadQueue => {
                let result = if state.demo {
                    Ok(demo_queue.upcoming().collect())
                } else if let Some(address) = soloist {
                    crate::soloist::queue(address)
                } else if let Some(api) = api.as_mut().filter(|_| state.connected && !state.soloist)
                {
                    api.queue().map_err(|error| error.to_string())
                } else {
                    Err("Liga um dispositivo para veres a fila.".into())
                };
                state.queue = Arc::default();
                state.queue_note = match result {
                    Ok(items) if items.is_empty() => "A fila está vazia.".into(),
                    Ok(items) => {
                        state.queue = items.into();
                        String::new()
                    }
                    Err(error) => error,
                };
                state.busy = false;
                emit(state.clone());
                continue;
            }
            _ => {}
        }
        let selected = if let Command::PlayTrack(ref input) = command {
            if state.demo {
                input
                    .strip_prefix("demo:")
                    .and_then(|i| i.parse::<usize>().ok())
                    .filter(|i| *i < DEMO_TRACKS.len())
                    .map(|i| model::demo_track(i).uri)
            } else {
                model::track_uri(input)
            }
        } else {
            None
        };
        if matches!(command, Command::PlayTrack(_)) && selected.is_none() {
            state.status = if state.demo {
                "Escolhe uma das faixas de exemplo.".into()
            } else {
                "Cola um link de faixa do Spotify ou um URI spotify:track:…".into()
            };
            state.busy = false;
            emit(state.clone());
            continue;
        }
        if let Command::Soloist {
            ref address,
            activate,
        } = command
        {
            soloist = None;
            activate_when_paired = activate;
            state = Snapshot {
                soloist: true,
                catalog_ready: catalog.is_some() || api.is_some() || library_api.is_some(),
                library: if state.demo {
                    Arc::new(Library {
                        ready: library_api.is_some(),
                        ..Library::default()
                    })
                } else {
                    state.library.clone()
                },
                results: if state.demo {
                    Arc::default()
                } else {
                    state.results.clone()
                },
                search_note: if state.demo {
                    String::new()
                } else {
                    state.search_note.clone()
                },
                ..Default::default()
            };
            match crate::soloist::endpoint(address) {
                Ok(addr) => soloist = Some(addr),
                Err(error) => {
                    state.status = error;
                    emit(state.clone());
                    continue;
                }
            }
        }
        let seek = match &command {
            Command::Seek {
                uri,
                device_id,
                position_ms,
            } if state.fresh && state.playback.can_seek() => state
                .playback
                .item
                .as_ref()
                .filter(|item| {
                    &item.uri == uri
                        && state.playback.device.as_ref().and_then(|d| d.id.as_ref())
                            == Some(device_id)
                })
                .map(|item| {
                    (
                        device_id.as_str(),
                        (*position_ms).min(item.duration_ms.saturating_sub(1)),
                    )
                }),
            _ => None,
        };
        if let Some(addr) = soloist {
            if matches!(command, Command::Logout | Command::Demo | Command::Login(_)) {
                soloist = None;
                activate_when_paired = false;
            } else {
                let action = match command {
                    Command::Toggle if state.fresh => Some(if state.playback.is_playing {
                        "pause"
                    } else {
                        "play"
                    }),
                    Command::Next if state.fresh => Some("skip_next"),
                    Command::Previous if state.fresh => Some("skip_prev"),
                    Command::Transfer(_) => Some("activate"),
                    _ => None,
                };
                let sent = selected
                    .as_deref()
                    .map(|uri| crate::soloist::play_track(addr, uri))
                    .or_else(|| seek.map(|(_, ms)| crate::soloist::seek(addr, ms)))
                    .or_else(|| action.map(|name| crate::soloist::request(addr, name)))
                    .or_else(|| match command {
                        Command::SetVolume(volume)
                            if state.fresh
                                && state
                                    .playback
                                    .device
                                    .as_ref()
                                    .and_then(Device::adjustable_volume)
                                    .is_some() =>
                        {
                            Some(crate::soloist::set_volume(addr, volume))
                        }
                        _ => None,
                    });
                if let Some(result) = sent {
                    match result {
                        Ok(_) => {
                            state.status =
                                "Pedido enviado ao Soloist. A atualizar a reprodução…".into();
                            state.busy = true;
                            next_poll = Instant::now() + Duration::from_millis(650);
                        }
                        Err(error) => {
                            state.status = error;
                            state.busy = false;
                        }
                    }
                    suspended = false;
                    emit(state.clone());
                    continue;
                }
                state.busy = false;
                let mut poll_delay = Duration::from_secs(3);
                match crate::soloist::request(addr, "get_state") {
                    Ok(value) => {
                        state.connected = true;
                        state.fresh = true;
                        state.playback = crate::soloist::playback(&value);
                        state.playback_updated_at = Some(Instant::now());
                        state.devices = state.playback.device.clone().into_iter().collect();
                        state.status = "Soloist ligado · reprodução local neste computador".into();
                        if std::mem::take(&mut activate_when_paired)
                            && value["is_active"] == false
                            && !cancelled.load(Ordering::Relaxed)
                        {
                            match crate::soloist::request(addr, "activate") {
                                Ok(_) => {
                                    state.status =
                                        "Pedido enviado ao Soloist. A atualizar a reprodução…"
                                            .into();
                                    state.busy = true;
                                    poll_delay = Duration::from_millis(650);
                                }
                                Err(error) => state.status = error,
                            }
                        }
                    }
                    Err(error) => {
                        let waiting_for_pairing = error == crate::soloist::PAIR_REQUIRED;
                        state.status = error;
                        state.fresh = false;
                        state.connected = false;
                        state.playback = Playback::default();
                        state.devices.clear();
                        state.queue = Arc::default();
                        if !waiting_for_pairing {
                            soloist = None;
                            activate_when_paired = false;
                        }
                    }
                }
                if let Some(note) = launch_note {
                    state.status.push(' ');
                    state.status.push_str(&note);
                }
                next_poll = Instant::now() + poll_delay;
                suspended = false;
                emit(state.clone());
                continue;
            }
        }
        let mut result: Result<(), ApiError> = Ok(());
        let mut refresh_devices = false;
        let mut fetch_playback = false;
        let mut confirm_change = false;
        let mut save_failed = false;
        let update_demo_queue = state.demo
            && !state.queue.is_empty()
            && matches!(
                command,
                Command::Next | Command::Previous | Command::PlayTrack(_) | Command::SetShuffle(_)
            );
        match command {
            Command::Login(id) => {
                let id = id.trim();
                api = SpotifyApi::restore(client.clone(), id, auth::SCOPES);
                state = Snapshot {
                    busy: true,
                    authorizing: api.is_none(),
                    catalog_ready: catalog.is_some() || library_api.is_some(),
                    library: if state.demo {
                        Arc::new(Library {
                            ready: library_api.is_some(),
                            ..Library::default()
                        })
                    } else {
                        state.library.clone()
                    },
                    status: if api.is_some() {
                        "A recuperar a autorização guardada."
                    } else {
                        "Autoriza a ligação no navegador. Podes cancelar aqui."
                    }
                    .into(),
                    ..Default::default()
                };
                emit(state.clone());
                if api.is_none() {
                    match auth::login(&client, id, &cancelled, auth::SCOPES) {
                        Ok(session) if !cancelled.load(Ordering::Relaxed) => {
                            let mut playback = SpotifyApi::new(client.clone(), id.into(), session);
                            save_failed = playback.remember().is_err();
                            api = Some(playback);
                        }
                        Ok(_) => state.status = "Início de sessão cancelado.".into(),
                        Err(e) => state.status = e.to_string(),
                    }
                }
                if api.is_some() {
                    state.connected = true;
                    suspended = false;
                    refresh_devices = true;
                    fetch_playback = true;
                }
            }
            Command::Logout => {
                drop(managed_soloist.take());
                let playback = api.take();
                catalog = catalog.or(playback);
                state = Snapshot {
                    status: "Reprodução desligada.".into(),
                    library: if state.demo {
                        Arc::new(Library {
                            ready: library_api.is_some(),
                            ..Library::default()
                        })
                    } else {
                        state.library.clone()
                    },
                    ..Default::default()
                };
            }
            Command::Demo => {
                api = None;
                demo_queue = DemoQueue::new(0, false);
                state = Snapshot {
                    demo: true,
                    library: Arc::new(library::demo()),
                    fresh: true,
                    catalog_ready: catalog.is_some(),
                    results: (0..DEMO_TRACKS.len())
                        .map(model::demo_track)
                        .collect::<Vec<_>>()
                        .into(),
                    playback: model::demo_playback(0, false),
                    queue: demo_queue.upcoming().collect(),
                    status: "Demonstração · dados fictícios · sem áudio ou pedidos à API".into(),
                    ..Default::default()
                };
                state.devices = vec![state.playback.device.clone().unwrap()];
                demo_updated_at = Instant::now();
            }
            _ if state.demo => match command {
                Command::Seek { .. } => {
                    if let Some((_, ms)) = seek {
                        state.playback.progress_ms = Some(ms);
                    }
                }
                Command::Toggle => state.playback.is_playing = !state.playback.is_playing,
                Command::Next | Command::Previous => {
                    demo_queue.step(matches!(command, Command::Next));
                    state.playback.item = Some(model::demo_track(demo_queue.current()));
                    state.playback.progress_ms = Some(0);
                }
                Command::PlayTrack(_) => {
                    if let Some(index) = selected
                        .as_deref()
                        .and_then(|uri| uri.strip_prefix("demo:"))
                        .and_then(|index| index.parse::<usize>().ok())
                    {
                        demo_queue.select(index);
                        state.playback.item = Some(model::demo_track(index));
                        state.playback.is_playing = true;
                        state.playback.progress_ms = Some(0);
                    }
                }
                Command::SetShuffle(enabled) => {
                    if state.playback.shuffle_state != Some(enabled) {
                        demo_queue = DemoQueue::new(demo_queue.current(), enabled);
                        state.playback.shuffle_state = Some(enabled);
                    }
                }
                Command::SetVolume(volume) => {
                    if let Some(device) = state.playback.device.as_mut() {
                        device.volume_percent = Some(volume.min(100));
                    }
                }
                _ => {}
            },
            Command::Refresh | Command::Poll if api.is_some() => {
                fetch_playback = true;
                refresh_devices = matches!(command, Command::Refresh);
                suspended = false;
            }
            Command::PlayTrack(_) if api.is_some() && state.connected => {
                if let Some(uri) = selected.as_deref() {
                    result = api.as_mut().unwrap().play_track(uri);
                    confirm_change = true;
                }
            }
            Command::PlayTrack(_) => {
                state.status =
                    "Liga o Soloist ou um dispositivo Spotify para ouvir a faixa.".into();
            }
            Command::Toggle if state.fresh && api.is_some() => {
                let action = if state.playback.is_playing {
                    "pausing"
                } else {
                    "resuming"
                };
                if state.playback.permits(action) {
                    result = api.as_mut().unwrap().play_pause(state.playback.is_playing);
                    confirm_change = true;
                }
            }
            Command::Next | Command::Previous if state.fresh && api.is_some() => {
                let next = matches!(command, Command::Next);
                if state.playback.permits(if next {
                    "skipping_next"
                } else {
                    "skipping_prev"
                }) {
                    result = api.as_mut().unwrap().skip(next);
                    confirm_change = true;
                }
            }
            Command::Seek { .. } => {
                if let (Some(api), Some((device_id, ms))) = (api.as_mut(), seek) {
                    result = api.seek(device_id, ms);
                    confirm_change = true;
                }
            }
            Command::SetShuffle(enabled) if state.fresh && state.playback.can_shuffle() => {
                if let (Some(api), Some(device_id)) = (
                    api.as_mut(),
                    state
                        .playback
                        .device
                        .as_ref()
                        .and_then(|device| device.id.as_deref()),
                ) {
                    result = api.set_shuffle(device_id, enabled);
                    confirm_change = true;
                }
            }
            Command::SetVolume(volume) if state.fresh => {
                if let (Some(api), Some(device)) = (api.as_mut(), state.playback.device.as_ref()) {
                    if let Some(id) = device
                        .id
                        .as_deref()
                        .filter(|_| device.adjustable_volume().is_some())
                    {
                        result = api.set_volume(id, volume);
                        confirm_change = true;
                    }
                }
            }
            Command::Transfer(index) if api.is_some() => {
                if let Some(device) = state.devices.get(index).filter(|d| !d.is_restricted) {
                    if let Some(ref id) = device.id {
                        result = api.as_mut().unwrap().transfer(id);
                        confirm_change = true;
                    }
                }
            }
            _ => {}
        }
        if update_demo_queue {
            state.queue = demo_queue.upcoming().collect();
        }
        if result.is_ok() && fetch_playback {
            if let Some(ref mut api) = api {
                result = api.playback().map(|playback| {
                    state.playback = playback;
                    state.playback_updated_at = Some(Instant::now());
                    state.fresh = true;
                    state.status = if state.playback.device.is_some() {
                        "Ligado ao Spotify · áudio no dispositivo selecionado"
                    } else {
                        "Abre o Spotify num dispositivo e inicia uma faixa. Depois, atualiza."
                    }
                    .into();
                });
                if result.is_ok() && refresh_devices {
                    result = api.devices().map(|devices| state.devices = devices);
                }
            }
        }
        let succeeded = result.is_ok();
        if let Err(error) = result {
            state.status = error.to_string();
            state.fresh = false;
            failures = failures.saturating_add(1);
            match error {
                ApiError::Unauthorized | ApiError::Auth(auth::AuthError::Expired) => {
                    api = None;
                    state.connected = false;
                    state.playback = Playback::default();
                    state.devices.clear();
                }
                ApiError::Forbidden | ApiError::QuotaExceeded => suspended = true,
                _ => {}
            }
        } else {
            failures = 0;
        }
        if save_failed {
            state
                .status
                .push_str(" Não foi possível guardar a autorização no cofre do sistema.");
        }
        state.busy = succeeded && confirm_change;
        state.authorizing = false;
        state.catalog_ready = catalog.is_some() || api.is_some() || library_api.is_some();
        if state.busy {
            state.status = "Pedido enviado. A confirmar o estado no Spotify…".into();
        }
        let interval = if failures > 0 {
            (60u64.saturating_mul(1 << failures.min(3))).min(300)
        } else if state.playback.is_playing {
            20
        } else {
            60
        };
        let delay = Duration::from_secs(if state.busy { 1 } else { interval })
            .max(api.as_ref().map(SpotifyApi::cooldown).unwrap_or_default());
        next_poll = Instant::now() + delay;
        emit(state.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use std::net::TcpListener;
    use std::sync::mpsc;
    use tungstenite::Message;

    #[test]
    fn startup_activates_soloist_once_after_pairing() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap().to_string();
        let server = std::thread::spawn(move || {
            // The launcher first checks whether the WebSocket port is already open.
            drop(listener.accept().unwrap().0);
            let inactive = json!({"type": "playback_state", "is_active": false});
            let replies = [
                (
                    "get_auth_state",
                    json!({"type": "auth_state", "logged_in": false}),
                ),
                (
                    "get_state",
                    json!({"type": "auth_state", "logged_in": false}),
                ),
                ("get_state", inactive.clone()),
                (
                    "activate",
                    json!({"type": "command_result", "command": "activate"}),
                ),
                (
                    "get_state",
                    json!({"type": "playback_state", "is_active": true}),
                ),
                ("get_state", inactive),
            ];
            for (command, reply) in replies {
                let (stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut socket = tungstenite::accept(stream).unwrap();
                let request = socket.read().unwrap();
                let request: Value = serde_json::from_str(request.to_text().unwrap()).unwrap();
                assert_eq!(request, json!({"type": "command", "command": command}));
                socket
                    .send(Message::Text(reply.to_string().into()))
                    .unwrap();
            }
        });
        let (tx, rx) = mpsc::sync_channel(4);
        let (events, updates) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            run(rx, Arc::new(AtomicBool::new(false)), |state| {
                events.send(state).unwrap();
            });
        });
        let snapshot = || updates.recv_timeout(Duration::from_secs(5)).unwrap();

        tx.send(Command::StartSoloist {
            address,
            executable: String::new(),
            key: String::new(),
            remember: false,
            activate: true,
        })
        .unwrap();
        assert!(snapshot().busy);
        assert_eq!(snapshot().status, crate::soloist::PAIR_REQUIRED);
        tx.send(Command::Refresh).unwrap();
        assert!(snapshot().busy);
        assert!(snapshot().playback.device.unwrap().is_active);
        tx.send(Command::Refresh).unwrap();
        let inactive = snapshot();
        assert!(!inactive.busy);
        assert_eq!(
            inactive.status,
            "Soloist ligado · reprodução local neste computador"
        );
        assert!(!inactive.playback.device.unwrap().is_active);

        tx.send(Command::Stop).unwrap();
        worker.join().unwrap();
        server.join().unwrap();
    }

    #[test]
    fn demo_seek_checks_the_target_and_advances_into_the_queue() {
        let (tx, rx) = mpsc::sync_channel(4);
        let (events, updates) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            run(rx, Arc::new(AtomicBool::new(false)), |state| {
                events.send(state).unwrap()
            })
        });
        let snapshot = || updates.recv_timeout(Duration::from_secs(3)).unwrap();
        tx.send(Command::Demo).unwrap();
        let initial = snapshot();
        let current = initial.playback.item.unwrap();
        for (uri, device_id, expected) in [
            ("demo:1", "demo", 72000),
            ("demo:0", "other", 72000),
            ("demo:0", "demo", 40000),
        ] {
            tx.send(Command::Seek {
                uri: uri.into(),
                device_id: device_id.into(),
                position_ms: 40000,
            })
            .unwrap();
            assert_eq!(snapshot().playback.progress_ms, Some(expected));
        }
        tx.send(Command::Seek {
            uri: current.uri,
            device_id: "demo".into(),
            position_ms: u64::MAX,
        })
        .unwrap();
        assert_eq!(
            snapshot().playback.progress_ms,
            Some(current.duration_ms - 1)
        );
        tx.send(Command::Toggle).unwrap();
        assert!(snapshot().playback.is_playing);
        let next = snapshot();
        assert_eq!(next.playback.item.unwrap().uri, initial.queue[0].uri);
        assert_eq!(next.queue[0].uri, initial.queue[1].uri);
        tx.send(Command::Stop).unwrap();
        worker.join().unwrap();
    }

    #[test]
    fn demo_controls_and_logout_work_without_credentials() {
        let (tx, rx) = mpsc::sync_channel(4);
        let (events, updates) = mpsc::channel();
        let handle = std::thread::spawn(move || {
            run(rx, Arc::new(AtomicBool::new(false)), |state| {
                events.send(state).unwrap();
            })
        });
        tx.send(Command::Demo).unwrap();
        let first = updates.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(first.demo && !first.connected && !first.playback.is_playing);
        tx.send(Command::OpenCollection("demo-noite".into(), 0))
            .unwrap();
        let playlist = updates.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(playlist.library.selection, "demo-noite");
        assert_eq!(playlist.library.tracks[0].name, "Luz de novembro");
        tx.send(Command::SetVolume(0)).unwrap();
        let quieter = updates.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(
            quieter.playback.device.unwrap().adjustable_volume(),
            Some(0)
        );
        tx.send(Command::Toggle).unwrap();
        assert!(
            updates
                .recv_timeout(Duration::from_secs(2))
                .unwrap()
                .playback
                .is_playing
        );
        tx.send(Command::Next).unwrap();
        assert_eq!(
            updates
                .recv_timeout(Duration::from_secs(2))
                .unwrap()
                .playback
                .item
                .unwrap()
                .name,
            "Luz de novembro"
        );
        tx.send(Command::Search("Maré Baixa".into())).unwrap();
        let search = updates.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(search.results.len(), 1);
        tx.send(Command::PlayTrack(search.results[0].uri.clone()))
            .unwrap();
        let chosen = updates
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
            .playback;
        assert!(chosen.is_playing);
        assert_eq!(chosen.item.unwrap().name, "Depois da chuva");
        assert_eq!(chosen.device.unwrap().adjustable_volume(), Some(0));
        tx.send(Command::LoadQueue).unwrap();
        let queued = updates.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(queued.queue[0].name, "Rua sem pressa");
        tx.send(Command::SetShuffle(true)).unwrap();
        let shuffled = updates.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(shuffled.playback.shuffle_state, Some(true));
        assert_eq!(shuffled.playback.item.unwrap().name, "Depois da chuva");
        let upcoming: std::collections::HashSet<_> = shuffled
            .queue
            .iter()
            .map(|track| track.uri.as_str())
            .collect();
        assert_eq!(upcoming.len(), DEMO_TRACKS.len() - 1);
        assert!(!upcoming.contains("demo:2"));
        tx.send(Command::Next).unwrap();
        let advanced = updates.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(advanced.playback.item.unwrap().uri, shuffled.queue[0].uri);
        tx.send(Command::Previous).unwrap();
        let previous = updates.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(previous.playback.item.unwrap().uri, "demo:2");
        tx.send(Command::SetShuffle(false)).unwrap();
        let sequential = updates.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(sequential.playback.shuffle_state, Some(false));
        assert_eq!(sequential.queue[0].uri, "demo:3");
        tx.send(Command::Logout).unwrap();
        let state = updates.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(
            !state.demo
                && !state.connected
                && state.playback.item.is_none()
                && state.devices.is_empty()
        );
        tx.send(Command::Stop).unwrap();
        handle.join().unwrap();
    }
}
