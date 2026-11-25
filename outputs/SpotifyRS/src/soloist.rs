use crate::model::{Artist, Device, Item, Playback};
use serde_json::{json, Value};
use std::{
    net::{SocketAddr, TcpStream},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tungstenite::{protocol::WebSocketConfig, Message};

pub const PAIR_REQUIRED: &str =
    "Seleciona o Soloist nos dispositivos da aplicação Spotify para emparelhar a conta.";

pub fn endpoint(value: &str) -> Result<SocketAddr, String> {
    let addr: SocketAddr = value
        .trim()
        .parse()
        .map_err(|_| "Usa um endereço local como 127.0.0.1:9090.".to_string())?;
    if !addr.ip().is_loopback() || addr.port() == 0 {
        return Err("O Soloist só pode ser ligado por loopback, com uma porta válida.".into());
    }
    Ok(addr)
}

pub fn request(addr: SocketAddr, command: &str) -> Result<Value, String> {
    exchange(addr, command, json!({"type":"command","command":command}))
}

pub fn play_track(addr: SocketAddr, uri: &str) -> Result<Value, String> {
    exchange(
        addr,
        "play",
        json!({"type":"command","command":"play","uri":uri}),
    )
}

pub fn seek(addr: SocketAddr, position_ms: u64) -> Result<Value, String> {
    exchange(
        addr,
        "seek",
        json!({"type":"command","command":"seek","position_ms":position_ms}),
    )
}

pub fn set_volume(addr: SocketAddr, volume: u8) -> Result<Value, String> {
    exchange(
        addr,
        "set_volume",
        json!({"type":"command","command":"set_volume","volume":volume.min(100)}),
    )
}

pub fn queue(addr: SocketAddr) -> Result<Vec<Item>, String> {
    let reply = exchange(
        addr,
        "get_queue",
        json!({"type":"command","command":"get_queue","limit":10}),
    )?;
    Ok(reply["upcoming"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let item = entity_item(&entry["item"])?;
            crate::model::track_uri(&item.uri)?;
            Some(item)
        })
        .take(10)
        .collect())
}

fn exchange(addr: SocketAddr, command: &str, body: Value) -> Result<Value, String> {
    let stream = TcpStream::connect_timeout(&addr, Duration::from_secs(2))
        .map_err(|_| "Soloist indisponível. Inicia-o com --ws 127.0.0.1:9090.".to_string())?;
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .map_err(|_| "Erro de ligação local.".to_string())?;
    stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .map_err(|_| "Erro de ligação local.".to_string())?;
    let config = WebSocketConfig::default()
        .max_message_size(Some(1048576))
        .max_frame_size(Some(1048576));
    let (mut socket, _) =
        tungstenite::client::client_with_config(format!("ws://{addr}"), stream, Some(config))
            .map_err(|_| "O serviço local não respondeu como Soloist WebSocket.".to_string())?;
    socket
        .send(Message::Text(body.to_string().into()))
        .map_err(|_| "Falhou o envio ao Soloist.".to_string())?;
    let deadline = Instant::now() + Duration::from_secs(4);
    while Instant::now() < deadline {
        let message = socket.read().map_err(|_| {
            "O Soloist desligou-se ou não respondeu. Tenta voltar a ligar.".to_string()
        })?;
        if let Message::Text(text) = message {
            let value: Value = serde_json::from_str(&text)
                .map_err(|_| "Resposta inválida do Soloist.".to_string())?;
            if value["type"] == "auth_state" && command == "get_auth_state" {
                let _ = socket.close(None);
                return Ok(value);
            }
            if value["type"] == "auth_state" && value["logged_in"] == false {
                return Err(PAIR_REQUIRED.into());
            }
            if value["type"] == "error" {
                return Err(
                    "O Soloist recusou o pedido. Confirma o emparelhamento e a ação disponível."
                        .into(),
                );
            }
            if (command == "get_state" && value["type"] == "playback_state")
                || (command == "get_queue" && value["type"] == "queue_changed")
                || (command != "get_queue"
                    && value["type"] == "command_result"
                    && value["command"] == command)
            {
                let _ = socket.close(None);
                return Ok(value);
            }
        }
    }
    Err("A resposta do Soloist excedeu o tempo limite.".into())
}

pub fn playback(v: &Value) -> Playback {
    let position = &v["position"];
    let progress_ms = position["position_ms"].as_u64().map(|anchor| {
        let elapsed = position["timestamp_ms"]
            .as_u64()
            .and_then(|timestamp| {
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .ok()?
                    .as_millis();
                Some(now.saturating_sub(u128::from(timestamp)) as u64)
            })
            .unwrap_or(0);
        let speed = position["speed"].as_f64().unwrap_or(0.0).max(0.0);
        anchor.saturating_add((elapsed as f64 * speed) as u64)
    });
    let mut p = Playback {
        is_playing: v["status"] == "playing",
        progress_ms,
        item: entity_item(&v["item"]),
        device: Some(Device {
            id: Some("soloist".into()),
            name: "Soloist · este computador".into(),
            is_active: v["is_active"] == true,
            is_restricted: v["is_active"] != true,
            supports_volume: true,
            volume_percent: v["volume"]
                .as_f64()
                .filter(|value| (0.0..=100.0).contains(value))
                .map(|value| value.round() as u8),
        }),
        ..Default::default()
    };
    for (local, remote) in [
        ("seeking", "seek"),
        ("pausing", "pause"),
        ("resuming", "play"),
        ("skipping_next", "skip_next"),
        ("skipping_prev", "skip_prev"),
    ] {
        p.actions
            .disallows
            .insert(local.into(), v["available_actions"].get(remote).is_none());
    }
    p
}

fn entity_item(entity: &Value) -> Option<Item> {
    let uri = entity["uri"].as_str().unwrap_or_default();
    let name = entity
        .pointer("/decorations/identity/name")
        .and_then(Value::as_str)
        .or_else(|| (!uri.is_empty()).then_some("Faixa Spotify"))?;
    let mut item = Item {
        uri: uri.into(),
        name: name.into(),
        duration_ms: entity
            .pointer("/decorations/playback/duration_ms")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        artists: entity
            .pointer("/decorations/creators")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|creator| {
                creator
                    .pointer("/entity/decorations/identity/name")
                    .and_then(Value::as_str)
                    .map(|name| Artist { name: name.into() })
            })
            .collect(),
        ..Default::default()
    };
    if let Some(uri) = crate::model::track_uri(uri) {
        item.external_urls.insert(
            "spotify".into(),
            format!(
                "https://open.spotify.com/track/{}",
                &uri["spotify:track:".len()..]
            ),
        );
    }
    Some(item)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_remote_endpoints() {
        assert!(endpoint("0.0.0.0:9090").is_err());
        assert!(endpoint("192.168.1.2:9090").is_err());
        assert!(endpoint("127.0.0.1:0").is_err());
        assert!(endpoint("127.0.0.1:9090").is_ok());
    }
    #[test]
    fn maps_state_and_permissions() {
        let p = playback(
            &json!({"status":"playing","is_active":true,"volume":63,"item":{"decorations":{"identity":{"name":"Teste"},"playback":{"duration_ms":1000}}},"available_actions":{"pause":{}}}),
        );
        assert!(p.is_playing);
        assert!(p.permits("pausing"));
        assert!(!p.permits("skipping_next"));
        assert_eq!(p.device.as_ref().unwrap().adjustable_volume(), Some(63));
        assert_eq!(p.item.unwrap().name, "Teste");
    }

    #[test]
    fn seek_requires_the_local_action_and_sends_an_absolute_position() {
        let mut state = json!({"is_active": true, "position":{"position_ms":12}, "item":{"uri":"track","decorations":{"playback":{"duration_ms":1500}}}, "available_actions":{}});
        assert!(!playback(&state).can_seek());
        state["available_actions"]["seek"] = json!({});
        assert!(playback(&state).can_seek());
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let mut ws = tungstenite::accept(listener.accept().unwrap().0).unwrap();
            let request: Value =
                serde_json::from_str(ws.read().unwrap().to_text().unwrap()).unwrap();
            assert_eq!(
                request,
                json!({"type":"command","command":"seek","position_ms":900})
            );
            ws.send(Message::Text(
                json!({"type":"command_result","command":"seek"})
                    .to_string()
                    .into(),
            ))
            .unwrap();
        });
        seek(addr, 900).unwrap();
        server.join().unwrap();
    }

    #[test]
    fn sends_volume_percentage_in_a_real_websocket_frame() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let mut ws = tungstenite::accept(listener.accept().unwrap().0).unwrap();
            let request: Value =
                serde_json::from_str(ws.read().unwrap().to_text().unwrap()).unwrap();
            assert_eq!(
                request,
                json!({"type":"command","command":"set_volume","volume":35})
            );
            ws.send(Message::Text(
                json!({"type":"command_result","command":"set_volume"})
                    .to_string()
                    .into(),
            ))
            .unwrap();
        });
        assert!(set_volume(addr, 35).is_ok());
        server.join().unwrap();
    }
    #[test]
    fn queue_waits_for_the_items_after_a_command_acknowledgement() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let mut ws = tungstenite::accept(listener.accept().unwrap().0).unwrap();
            let request: Value =
                serde_json::from_str(ws.read().unwrap().to_text().unwrap()).unwrap();
            assert_eq!(request["command"], "get_queue");
            for reply in [
                json!({"type":"command_result","command":"get_queue"}),
                json!({"type":"queue_changed","upcoming":[{"item":{"uri":"spotify:track:6rqhFgbbKwnb9MLmUQDhG6","decorations":{"identity":{"name":"Following track"}}}}]}),
            ] {
                ws.send(Message::Text(reply.to_string().into())).unwrap();
            }
        });
        assert_eq!(queue(addr).unwrap()[0].name, "Following track");
        server.join().unwrap();
    }

    #[test]
    fn sends_selected_track_uri_in_a_real_websocket_frame() {
        const URI: &str = "spotify:track:6rqhFgbbKwnb9MLmUQDhG6";
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let mut ws = tungstenite::accept(listener.accept().unwrap().0).unwrap();
            let v: Value = serde_json::from_str(ws.read().unwrap().to_text().unwrap()).unwrap();
            assert_eq!(v, json!({"type":"command","command":"play","uri":URI}));
            ws.send(Message::Text(
                json!({"type":"command_result","command":"play"})
                    .to_string()
                    .into(),
            ))
            .unwrap();
        });
        assert!(play_track(addr, URI).is_ok());
        server.join().unwrap();
    }
}
