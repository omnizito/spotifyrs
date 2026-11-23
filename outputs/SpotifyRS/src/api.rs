use crate::{
    auth::{self, Session},
    library::{self, Cover, Playlist, PAGE_SIZE},
    model::{track_uri, Device, Item, Playback},
};
use reqwest::{blocking::Client, Method, StatusCode};
use serde::{de::DeserializeOwned, Deserialize};
use serde_json::{json, Value};
use std::{
    io::{Cursor, Read},
    time::{Duration, Instant},
};

#[derive(Deserialize)]
struct Page<T> {
    items: Vec<Option<T>>,
    next: Option<String>,
}

#[derive(Deserialize)]
struct TrackEntry {
    item: Option<Item>,
    track: Option<Item>,
}

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("A sessão expirou. Volta a iniciar sessão.")]
    Unauthorized,
    #[error("Acesso recusado. Confirma Premium, permissões e utilizadores autorizados no painel Spotify.")]
    Forbidden,
    #[error(
        "Não existe um dispositivo disponível. Abre o Spotify num dispositivo e inicia uma faixa."
    )]
    NoDevice,
    #[error("O Spotify pediu uma pausa. Tenta novamente dentro de {0} segundos.")]
    RateLimited(u64),
    #[error("A quota da aplicação foi esgotada. Consulta o painel Spotify; as atualizações estão suspensas.")]
    QuotaExceeded,
    #[error("O Spotify devolveu um erro (HTTP {0}).")]
    Http(u16),
    #[error("Sem ligação ao Spotify. Verifica a rede e tenta atualizar.")]
    Network,
    #[error("A resposta do Spotify não é válida ou excede o limite de memória.")]
    Response,
    #[error(transparent)]
    Auth(#[from] auth::AuthError),
}

pub fn http_client() -> Result<Client, reqwest::Error> {
    Client::builder()
        .timeout(Duration::from_secs(12))
        .connect_timeout(Duration::from_secs(5))
        .pool_max_idle_per_host(1)
        .redirect(reqwest::redirect::Policy::none())
        .user_agent("SpotifyRS-prototype/0.1")
        .build()
}

pub struct SpotifyApi {
    client: Client,
    client_id: String,
    session: Session,
    remember_session: bool,
    base: String,
    retry_at: Option<Instant>,
    quota_exceeded: bool,
}

impl SpotifyApi {
    pub fn new(client: Client, client_id: String, session: Session) -> Self {
        Self {
            client,
            client_id,
            session,
            remember_session: false,
            base: "https://api.spotify.com/v1".into(),
            retry_at: None,
            quota_exceeded: false,
        }
    }

    pub fn restore(client: Client, client_id: &str, scopes: &'static str) -> Option<Self> {
        let mut api = Self::new(
            client,
            client_id.into(),
            Session {
                access_token: String::new(),
                refresh_token: auth::saved_token(client_id, scopes)?,
                expires_at: Instant::now(),
                scopes,
            },
        );
        api.remember_session = true;
        Some(api)
    }

    pub fn remember(&mut self) -> std::io::Result<()> {
        auth::save_session(&self.client_id, &self.session)?;
        self.remember_session = true;
        Ok(())
    }

    fn refresh_session(&mut self) -> Result<(), auth::AuthError> {
        let scopes = self.session.scopes;
        if self.remember_session {
            if let Some(token) = auth::saved_token(&self.client_id, scopes) {
                self.session.refresh_token = token;
            }
        }
        let result = auth::refresh(&self.client, &self.client_id, &self.session);
        let same_credential = self.remember_session
            && auth::saved_token(&self.client_id, scopes).as_ref()
                == Some(&self.session.refresh_token);
        match result {
            Ok(session) => {
                if same_credential && self.session.refresh_token != session.refresh_token {
                    self.remember_session = auth::save_session(&self.client_id, &session).is_ok();
                }
                self.session = session;
                Ok(())
            }
            Err(error) => {
                if same_credential && matches!(error, auth::AuthError::Expired) {
                    let _ = auth::forget_session(scopes);
                }
                Err(error)
            }
        }
    }

    pub fn cooldown(&self) -> Duration {
        self.retry_at
            .map(|at| at.saturating_duration_since(Instant::now()))
            .unwrap_or_default()
    }

    fn request(
        &mut self,
        method: Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<Option<Value>, ApiError> {
        if self.quota_exceeded {
            return Err(ApiError::QuotaExceeded);
        }
        let remaining = self.cooldown();
        if !remaining.is_zero() {
            return Err(ApiError::RateLimited(remaining.as_secs() + 1));
        }
        if Instant::now() >= self.session.expires_at {
            self.refresh_session()?;
        }
        let mut request = self
            .client
            .request(method, format!("{}{path}", self.base))
            .bearer_auth(&self.session.access_token);
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request.send().map_err(|_| ApiError::Network)?;
        let status = response.status();
        let retry_seconds = response
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u32>().ok())
            .unwrap_or(60)
            .max(1) as u64;
        // Impõe a espera antes de ler o corpo: uma falha de rede não pode apagar Retry-After.
        if status == StatusCode::TOO_MANY_REQUESTS {
            self.retry_at = Some(Instant::now() + Duration::from_secs(retry_seconds));
        }
        let mut bytes = Vec::new();
        response
            .take(1_048_577)
            .read_to_end(&mut bytes)
            .map_err(|_| ApiError::Network)?;
        if bytes.len() > 1_048_576 {
            return Err(ApiError::Response);
        }
        match status {
            StatusCode::UNAUTHORIZED => return Err(ApiError::Unauthorized),
            StatusCode::FORBIDDEN => return Err(ApiError::Forbidden),
            StatusCode::NOT_FOUND => return Err(ApiError::NoDevice),
            StatusCode::TOO_MANY_REQUESTS => {
                if serde_json::from_slice::<Value>(&bytes)
                    .ok()
                    .and_then(|v| {
                        v.pointer("/error/reason")
                            .and_then(Value::as_str)
                            .map(str::to_owned)
                    })
                    .as_deref()
                    == Some("QUOTA_EXCEEDED")
                {
                    self.quota_exceeded = true;
                    return Err(ApiError::QuotaExceeded);
                }
                return Err(ApiError::RateLimited(retry_seconds));
            }
            _ if !status.is_success() => return Err(ApiError::Http(status.as_u16())),
            _ => {}
        }
        if status == StatusCode::NO_CONTENT || bytes.is_empty() {
            return Ok(None);
        }
        serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|_| ApiError::Response)
    }

    fn get<T: DeserializeOwned>(&mut self, path: &str) -> Result<Option<T>, ApiError> {
        self.request(Method::GET, path, None)?
            .map(|v| serde_json::from_value(v).map_err(|_| ApiError::Response))
            .transpose()
    }

    pub fn playback(&mut self) -> Result<Playback, ApiError> {
        Ok(self.get("/me/player")?.unwrap_or_default())
    }

    pub fn playlists(&mut self, offset: usize) -> Result<(Vec<Playlist>, bool), ApiError> {
        let page = self
            .get::<Page<Playlist>>(&format!("/me/playlists?limit={PAGE_SIZE}&offset={offset}"))?
            .ok_or(ApiError::Response)?;
        Ok((
            page.items
                .into_iter()
                .flatten()
                .filter(|playlist| library::valid_id(&playlist.id) && !playlist.name.is_empty())
                .collect(),
            page.next.is_some(),
        ))
    }

    pub fn library_tracks(
        &mut self,
        id: &str,
        offset: usize,
    ) -> Result<(Vec<Item>, bool), ApiError> {
        let path = if id == library::SAVED {
            format!("/me/tracks?limit={PAGE_SIZE}&offset={offset}")
        } else if library::valid_id(id) {
            format!("/playlists/{id}/items?limit={PAGE_SIZE}&offset={offset}")
        } else {
            return Err(ApiError::Response);
        };
        let page = self
            .get::<Page<TrackEntry>>(&path)?
            .ok_or(ApiError::Response)?;
        Ok((
            page.items
                .into_iter()
                .flatten()
                .filter_map(|entry| entry.item.or(entry.track))
                .filter(|item| !item.name.is_empty() && track_uri(&item.uri).is_some())
                .collect(),
            page.next.is_some(),
        ))
    }

    pub fn cover(client: &Client, input: &str) -> Option<Cover> {
        let url = url::Url::parse(input).ok()?;
        if url.scheme() != "https"
            || !matches!(
                url.host_str()?,
                "i.scdn.co"
                    | "mosaic.scdn.co"
                    | "image-cdn-ak.spotifycdn.com"
                    | "image-cdn-fa.spotifycdn.com"
            )
            || !url.username().is_empty()
            || url.password().is_some()
            || url.port().is_some()
        {
            return None;
        }
        let response = client
            .get(url)
            .timeout(Duration::from_millis(900))
            .send()
            .ok()?
            .error_for_status()
            .ok()?;
        let mut data = Vec::new();
        response.take(524_289).read_to_end(&mut data).ok()?;
        if data.len() > 524_288 {
            return None;
        }
        let mut reader = image::ImageReader::new(Cursor::new(data))
            .with_guessed_format()
            .ok()?;
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(1200);
        limits.max_image_height = Some(1200);
        limits.max_alloc = Some(12 * 1024 * 1024);
        reader.limits(limits);
        let image = reader.decode().ok()?.thumbnail(200, 200).into_rgba8();
        Some(Cover {
            width: image.width(),
            height: image.height(),
            pixels: image.into_raw(),
        })
    }

    pub fn search_tracks(&mut self, query: &str) -> Result<Vec<Item>, ApiError> {
        #[derive(Deserialize)]
        struct Search {
            tracks: Tracks,
        }
        #[derive(Deserialize)]
        struct Tracks {
            items: Vec<Option<Item>>,
        }
        let query = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("q", query)
            .append_pair("type", "track")
            .append_pair("limit", "10")
            .finish();
        let data = self
            .get::<Search>(&format!("/search?{query}"))?
            .ok_or(ApiError::Response)?;
        Ok(data
            .tracks
            .items
            .into_iter()
            .flatten()
            .filter(|item| !item.name.is_empty() && track_uri(&item.uri).is_some())
            .take(10)
            .collect())
    }

    pub fn play_track(&mut self, uri: &str) -> Result<(), ApiError> {
        self.request(Method::PUT, "/me/player/play", Some(json!({"uris": [uri]})))
            .map(|_| ())
    }

    pub fn devices(&mut self) -> Result<Vec<Device>, ApiError> {
        #[derive(Deserialize)]
        struct Devices {
            devices: Vec<Device>,
        }
        Ok(self
            .get::<Devices>("/me/player/devices")?
            .map(|v| v.devices.into_iter().take(32).collect())
            .unwrap_or_default())
    }

    pub fn play_pause(&mut self, currently_playing: bool) -> Result<(), ApiError> {
        self.request(
            Method::PUT,
            if currently_playing {
                "/me/player/pause"
            } else {
                "/me/player/play"
            },
            None,
        )
        .map(|_| ())
    }

    pub fn skip(&mut self, next: bool) -> Result<(), ApiError> {
        // Nunca repetir automaticamente: duas chamadas podem saltar duas faixas.
        self.request(
            Method::POST,
            if next {
                "/me/player/next"
            } else {
                "/me/player/previous"
            },
            None,
        )
        .map(|_| ())
    }

    pub fn set_shuffle(&mut self, device_id: &str, enabled: bool) -> Result<(), ApiError> {
        let query = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("state", if enabled { "true" } else { "false" })
            .append_pair("device_id", device_id)
            .finish();
        self.request(Method::PUT, &format!("/me/player/shuffle?{query}"), None)
            .map(|_| ())
    }

    pub fn set_volume(&mut self, device_id: &str, volume: u8) -> Result<(), ApiError> {
        let query = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("volume_percent", &volume.min(100).to_string())
            .append_pair("device_id", device_id)
            .finish();
        self.request(Method::PUT, &format!("/me/player/volume?{query}"), None)
            .map(|_| ())
    }

    pub fn transfer(&mut self, id: &str) -> Result<(), ApiError> {
        self.request(
            Method::PUT,
            "/me/player",
            Some(json!({"device_ids": [id], "play": false})),
        )
        .map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{io::Write, net::TcpListener};

    fn mock(
        status: &str,
        headers: &str,
        body: &str,
    ) -> (SpotifyApi, std::thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n{headers}\r\n{body}",
            body.len()
        );
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut bytes = [0; 8192];
            let n = stream.read(&mut bytes).unwrap();
            stream.write_all(response.as_bytes()).unwrap();
            String::from_utf8_lossy(&bytes[..n]).into_owned()
        });
        let session = Session {
            scopes: auth::SCOPES,
            access_token: "test-only".into(),
            refresh_token: "test-only".into(),
            expires_at: Instant::now() + Duration::from_secs(3600),
        };
        let mut api = SpotifyApi::new(
            Client::builder()
                .no_proxy()
                .timeout(Duration::from_secs(3))
                .build()
                .unwrap(),
            "test".into(),
            session,
        );
        api.base = base;
        (api, handle)
    }

    #[test]
    fn no_content_means_no_active_playback() {
        let (mut api, server) = mock("204 No Content", "", "");
        assert!(api.playback().unwrap().item.is_none());
        assert!(server.join().unwrap().starts_with("GET /me/player "));
    }

    #[test]
    fn lists_playlists_and_preserves_pagination() {
        let (mut api, server) = mock(
            "200 OK",
            "",
            r#"{"items":[null,{"id":"6rqhFgbbKwnb9MLmUQDhG6","name":"Viagem","images":null}],"next":"next"}"#,
        );
        let (playlists, more) = api.playlists(20).unwrap();
        assert!(more);
        assert_eq!(playlists[0].name, "Viagem");
        assert!(server
            .join()
            .unwrap()
            .starts_with("GET /me/playlists?limit=20&offset=20 "));
    }

    #[test]
    fn handles_both_playlist_and_saved_track_response_shapes() {
        let id = "6rqhFgbbKwnb9MLmUQDhG6";
        for (collection, field, path) in [
            (id, "item", format!("/playlists/{id}/items")),
            (library::SAVED, "track", "/me/tracks".into()),
        ] {
            let track = json!({"name": "Música", "uri": format!("spotify:track:{id}")});
            let body = json!({"items": [null, {field: track}], "next": null}).to_string();
            let (mut api, server) = mock("200 OK", "", &body);
            let (tracks, more) = api.library_tracks(collection, 0).unwrap();
            assert!(!more);
            assert_eq!(tracks.len(), 1);
            assert_eq!(tracks[0].name, "Música");
            assert!(server
                .join()
                .unwrap()
                .starts_with(&format!("GET {path}?limit=20&offset=0 ")));
        }
    }

    #[test]
    fn shuffle_sends_explicit_state_to_the_confirmed_device() {
        for enabled in [true, false] {
            let (mut api, server) = mock("204 No Content", "", "");
            api.set_shuffle("device+1", enabled).unwrap();
            assert!(server.join().unwrap().starts_with(&format!(
                "PUT /me/player/shuffle?state={enabled}&device_id=device%2B1 "
            )));
        }
    }

    #[test]
    fn volume_targets_the_confirmed_device() {
        let (mut api, server) = mock("204 No Content", "", "");
        api.set_volume("device+1", 42).unwrap();
        assert!(server
            .join()
            .unwrap()
            .starts_with("PUT /me/player/volume?volume_percent=42&device_id=device%2B1 "));
    }

    #[test]
    fn search_encodes_query_and_keeps_only_tracks_with_valid_identifiers() {
        let (mut api, server) = mock(
            "200 OK",
            "",
            r#"{"tracks":{"items":[null,
            {"name":"Valid","uri":"spotify:track:6rqhFgbbKwnb9MLmUQDhG6"},
            {"name":"Local only","uri":"spotify:local:artist:album:track:60"}
        ]}}"#,
        );
        let tracks = api.search_tracks("Maré & luz").unwrap();
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].name, "Valid");
        assert!(server
            .join()
            .unwrap()
            .starts_with("GET /search?q=Mar%C3%A9+%26+luz&type=track&limit=10 "));
    }
    #[test]
    fn rate_limit_blocks_followup_without_network_request() {
        let (mut api, server) = mock("429 Too Many Requests", "Retry-After: 120\r\n", "{}");
        assert!(matches!(api.playback(), Err(ApiError::RateLimited(120))));
        server.join().unwrap();
        assert!(matches!(api.devices(), Err(ApiError::RateLimited(_))));
        assert!(api.cooldown() > Duration::from_secs(118));
    }
    #[test]
    fn quota_exceeded_suspends_requests() {
        let (mut api, server) = mock(
            "429 Too Many Requests",
            "",
            r#"{"error":{"reason":"QUOTA_EXCEEDED"}}"#,
        );
        assert!(matches!(api.playback(), Err(ApiError::QuotaExceeded)));
        server.join().unwrap();
        assert!(matches!(api.devices(), Err(ApiError::QuotaExceeded)));
    }
    #[test]
    fn distinguishes_permission_and_expired_session() {
        for (status, code) in [
            ("401 Unauthorized", 401),
            ("403 Forbidden", 403),
            ("404 Not Found", 404),
        ] {
            let (mut api, server) = mock(status, "", "{}");
            let err = api.playback().unwrap_err();
            assert!(matches!(
                (code, err),
                (401, ApiError::Unauthorized)
                    | (403, ApiError::Forbidden)
                    | (404, ApiError::NoDevice)
            ));
            server.join().unwrap();
        }
    }
}
