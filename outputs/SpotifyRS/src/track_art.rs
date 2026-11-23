use crate::{AppWindow, TrackRow};
use slint::{ComponentHandle, Image, Model, ModelRc, SharedPixelBuffer, SharedString};
use spotifyrs::api::{http_client, SpotifyApi};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet, VecDeque},
    io,
    sync::mpsc::{sync_channel, SyncSender},
};

const CACHE_SIZE: usize = 96;

#[derive(Default)]
struct Artwork {
    sender: Option<SyncSender<String>>,
    pending: HashSet<String>,
    images: HashMap<String, Image>,
    order: VecDeque<String>,
}

thread_local! {
    static ARTWORK: RefCell<Artwork> = RefCell::default();
}

pub fn image(url: &str) -> Image {
    if url.is_empty() {
        return Image::default();
    }
    ARTWORK.with_borrow_mut(|artwork| {
        if let Some(image) = artwork.images.get(url) {
            return image.clone();
        }
        if !artwork.pending.contains(url)
            && artwork
                .sender
                .as_ref()
                .is_some_and(|sender| sender.try_send(url.to_owned()).is_ok())
        {
            artwork.pending.insert(url.to_owned());
        }
        Image::default()
    })
}

pub fn start(ui: &AppWindow) -> io::Result<()> {
    let (sender, requests) = sync_channel::<String>(64);
    let weak = ui.as_weak();
    std::thread::Builder::new()
        .name("track-artwork".into())
        .spawn(move || {
            let Ok(client) = http_client() else { return };
            for url in requests {
                let cover = SpotifyApi::cover(&client, &url);
                if weak
                    .upgrade_in_event_loop(move |ui| {
                        let image = cover.map_or_else(Image::default, |cover| {
                            Image::from_rgba8(SharedPixelBuffer::clone_from_slice(
                                &cover.pixels,
                                cover.width,
                                cover.height,
                            ))
                        });
                        ARTWORK.with_borrow_mut(|artwork| {
                            artwork.pending.remove(&url);
                            if artwork.order.len() == CACHE_SIZE {
                                if let Some(oldest) = artwork.order.pop_front() {
                                    artwork.images.remove(&oldest);
                                }
                            }
                            artwork.order.push_back(url.clone());
                            artwork.images.insert(url.clone(), image.clone());
                        });
                        if ui.get_track_art_url().as_str() == url {
                            ui.set_track_art(image.clone());
                        }
                        for model in track_models(&ui) {
                            for index in 0..model.row_count() {
                                if let Some(mut row) = model.row_data(index) {
                                    if row.art_url.as_str() == url {
                                        row.art = image.clone();
                                        model.set_row_data(index, row);
                                    }
                                }
                            }
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        })?;
    ARTWORK.with_borrow_mut(|artwork| artwork.sender = Some(sender));
    Ok(())
}

fn track_models(ui: &AppWindow) -> [ModelRc<TrackRow>; 4] {
    [
        ui.get_tracks(),
        ui.get_queue(),
        ui.get_collection_tracks(),
        ui.get_saved_preview(),
    ]
}

pub fn known_url(ui: &AppWindow, uri: &str) -> SharedString {
    track_models(ui)
        .iter()
        .flat_map(|model| model.iter())
        .find(|row| row.uri.as_str() == uri && !row.art_url.is_empty())
        .map_or_else(SharedString::default, |row| row.art_url)
}
