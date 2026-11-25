use serde::Deserialize;
use std::collections::HashMap;

pub const DEMO_TRACKS: [(&str, &str, u64); 6] = [
    ("Horizonte tranquilo", "Linha Clara", 224_000),
    ("Luz de novembro", "Estação Norte", 188_000),
    ("Depois da chuva", "Maré Baixa", 212_000),
    ("Rua sem pressa", "Linha Clara", 198_000),
    ("Entre janelas", "Ponto de Encontro", 245_000),
    ("Último eléctrico", "Estação Norte", 173_000),
];

#[derive(Clone, Debug, Default, Deserialize)]
pub struct Device {
    pub id: Option<String>,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub is_active: bool,
    #[serde(default)]
    pub is_restricted: bool,
    #[serde(default)]
    pub supports_volume: bool,
    pub volume_percent: Option<u8>,
}

impl Device {
    pub fn adjustable_volume(&self) -> Option<u8> {
        self.volume_percent
            .filter(|&volume| volume <= 100 && self.supports_volume && !self.is_restricted)
            .filter(|_| self.id.as_ref().is_some_and(|id| !id.is_empty()))
    }
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
pub struct Artist {
    pub name: String,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
pub struct Album {
    #[serde(default)]
    images: Vec<AlbumImage>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
struct AlbumImage {
    url: String,
    width: Option<u32>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
pub struct Item {
    #[serde(default)]
    pub uri: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub artists: Vec<Artist>,
    #[serde(default)]
    pub duration_ms: u64,
    #[serde(default)]
    pub external_urls: HashMap<String, String>,
    pub album: Option<Album>,
}

impl Item {
    pub fn artwork_url(&self) -> &str {
        self.album
            .iter()
            .flat_map(|album| &album.images)
            .min_by_key(|image| image.width.unwrap_or(300).abs_diff(160))
            .map_or("", |image| image.url.as_str())
    }

    pub fn artist_line(&self) -> String {
        self.artists
            .iter()
            .map(|a| a.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

pub fn track_uri(value: &str) -> Option<String> {
    let value = value.trim();
    let url;
    let id = if let Some(id) = value.strip_prefix("spotify:track:") {
        id
    } else {
        url = url::Url::parse(value).ok()?;
        if url.scheme() != "https"
            || url.host_str() != Some("open.spotify.com")
            || !url.username().is_empty()
            || url.password().is_some()
            || url.port().is_some()
        {
            return None;
        }
        let mut parts = url.path_segments()?;
        let mut kind = parts.next()?;
        if kind.starts_with("intl-") {
            kind = parts.next()?;
        }
        let id = parts.next()?;
        if kind != "track" || parts.next().is_some() {
            return None;
        }
        id
    };
    (id.len() == 22 && id.bytes().all(|b| b.is_ascii_alphanumeric()))
        .then(|| format!("spotify:track:{id}"))
}

pub fn demo_track(index: usize) -> Item {
    let index = index % DEMO_TRACKS.len();
    let (name, artist, duration_ms) = DEMO_TRACKS[index];
    Item {
        uri: format!("demo:{index}"),
        name: name.into(),
        artists: vec![Artist {
            name: artist.into(),
        }],
        duration_ms,
        ..Default::default()
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct Actions {
    #[serde(default)]
    pub disallows: HashMap<String, bool>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct Playback {
    #[serde(default)]
    pub is_playing: bool,
    pub shuffle_state: Option<bool>,
    pub progress_ms: Option<u64>,
    pub item: Option<Item>,
    pub device: Option<Device>,
    #[serde(default)]
    pub actions: Actions,
}

impl Playback {
    pub fn can_seek(&self) -> bool {
        self.progress_ms.is_some()
            && self.permits("seeking")
            && self
                .item
                .as_ref()
                .is_some_and(|item| item.duration_ms > 0 && !item.uri.is_empty())
            && self.device.as_ref().is_some_and(|device| {
                device.is_active && device.id.as_deref().is_some_and(|id| !id.is_empty())
            })
    }

    pub fn can_shuffle(&self) -> bool {
        self.shuffle_state.is_some()
            && self.permits("toggling_shuffle")
            && self
                .device
                .as_ref()
                .is_some_and(|device| device.id.is_some())
    }

    pub fn permits(&self, action: &str) -> bool {
        self.device.as_ref().is_some_and(|d| !d.is_restricted)
            && !self.actions.disallows.get(action).copied().unwrap_or(false)
    }
}

pub fn clock(ms: u64) -> String {
    let s = ms / 1000;
    format!("{}:{:02}", s / 60, s % 60)
}

pub fn demo_playback(index: usize, playing: bool) -> Playback {
    Playback {
        is_playing: playing,
        shuffle_state: Some(false),
        progress_ms: Some(72_000),
        item: Some(demo_track(index)),
        device: Some(Device {
            id: Some("demo".into()),
            name: "Dispositivo de exemplo".into(),
            is_active: true,
            is_restricted: false,
            supports_volume: true,
            volume_percent: Some(65),
        }),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn accepts_null_item_and_absent_optional_fields() {
        let p: Playback =
            serde_json::from_str(r#"{"item":null,"is_playing":false,"progress_ms":null}"#).unwrap();
        assert!(p.item.is_none());
        assert!(!p.permits("resuming"));
    }
    #[test]
    fn restricted_device_and_disallowed_actions_are_respected() {
        let mut p = demo_playback(0, true);
        p.actions.disallows.insert("pausing".into(), true);
        assert!(!p.permits("pausing"));
        assert!(p.permits("skipping_next"));
        p.device.as_mut().unwrap().is_restricted = true;
        assert!(!p.permits("skipping_next"));
    }
    #[test]
    fn formats_minutes_without_hour_wrap() {
        assert_eq!(clock(3_661_000), "61:01");
    }

    #[test]
    fn normalizes_shared_track_links_without_accepting_other_hosts_or_content() {
        let uri = "spotify:track:6rqhFgbbKwnb9MLmUQDhG6";
        assert_eq!(track_uri(uri).as_deref(), Some(uri));
        assert_eq!(
            track_uri("https://open.spotify.com/intl-pt/track/6rqhFgbbKwnb9MLmUQDhG6?si=share")
                .as_deref(),
            Some(uri)
        );
        assert!(
            track_uri("https://open.spotify.com.evil.test/track/6rqhFgbbKwnb9MLmUQDhG6").is_none()
        );
        assert!(track_uri("spotify:playlist:6rqhFgbbKwnb9MLmUQDhG6").is_none());
    }
}
