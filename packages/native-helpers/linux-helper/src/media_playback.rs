//! Best-effort MPRIS pause/resume, independent of the compositor and input backend.
use std::collections::{HashMap, HashSet};
use std::time::Duration;

use futures_util::{future::join_all, StreamExt};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tokio::time::timeout;
use zbus::proxy::{Builder, CacheProperties, SignalStream};
use zbus::zvariant::OwnedValue;
use zbus::{Connection, Proxy};

const PREFIX: &str = "org.mpris.MediaPlayer2.";
const PATH: &str = "/org/mpris/MediaPlayer2";
const PLAYER: &str = "org.mpris.MediaPlayer2.Player";
const PROPERTIES: &str = "org.freedesktop.DBus.Properties";
const OPERATION_TIMEOUT: Duration = Duration::from_millis(500);
type Properties = HashMap<String, OwnedValue>;

#[derive(Default)]
pub struct MediaPlaybackService {
    paused: Vec<PausedPlayer>,
}

impl MediaPlaybackService {
    pub async fn pause(&mut self) {
        let connection = match timeout(OPERATION_TIMEOUT, Connection::session()).await {
            Ok(Ok(connection)) => connection,
            _ => {
                eprintln!("[media] session bus unavailable; keeping audio mute fallback");
                return;
            }
        };
        self.pause_on(&connection).await;
    }

    async fn pause_on(&mut self, connection: &Connection) {
        let names = timeout(OPERATION_TIMEOUT, async {
            zbus::fdo::DBusProxy::new(connection)
                .await?
                .list_names()
                .await
        })
        .await;
        let Ok(Ok(names)) = names else { return };
        // Prepare concurrently so one unresponsive player cannot hold up others.
        let players = join_all(
            names
                .into_iter()
                .filter(|name| name.starts_with(PREFIX))
                .map(|name| async move {
                    timeout(
                        OPERATION_TIMEOUT,
                        PausedPlayer::prepare(connection, name.as_str()),
                    )
                    .await
                    .ok()
                    .and_then(Result::ok)
                    .flatten()
                }),
        )
        .await;
        let mut owners = HashSet::new();
        let paused = join_all(
            players
                .into_iter()
                .flatten()
                .filter(|player| owners.insert(player.player.destination().to_string()))
                .map(|player| async move {
                    // A timed-out Pause might already have reached the player. Retain its
                    // snapshot and watcher so stop can still restore that exact session.
                    match timeout(
                        OPERATION_TIMEOUT,
                        player.player.call::<_, _, ()>("Pause", &()),
                    )
                    .await
                    {
                        Ok(Err(error)) => {
                            eprintln!("[media] player rejected Pause: {error}");
                            None
                        }
                        _ => Some(player),
                    }
                }),
        )
        .await;
        self.paused.extend(paused.into_iter().flatten());
        if !self.paused.is_empty() {
            eprintln!(
                "[media] paused {} player(s) for dictation",
                self.paused.len()
            );
        }
    }

    pub async fn resume(&mut self) {
        join_all(self.paused.drain(..).map(|mut player| async move {
            let result = timeout(OPERATION_TIMEOUT, player.resume()).await;
            if !matches!(result, Ok(Ok(()))) {
                eprintln!("[media] could not restore a player; continuing dictation cleanup");
            }
        }))
        .await;
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Track(Vec<(String, String)>);

impl Track {
    fn from_metadata(metadata: &Properties) -> Self {
        let mut keys = vec!["mpris:trackid", "xesam:url"];
        if !keys.iter().any(|key| metadata.contains_key(*key)) {
            keys.push("xesam:title");
        }
        Self(
            keys.into_iter()
                .filter_map(|key| {
                    metadata
                        .get(key)
                        .map(|value| (key.to_owned(), format!("{value:?}")))
                })
                .collect(),
        )
    }
}

struct Snapshot {
    status: String,
    can_control: bool,
    can_pause: bool,
    can_play: bool,
    track: Track,
}

impl Snapshot {
    async fn read(properties: &Proxy<'_>) -> zbus::Result<Self> {
        let mut values: Properties = properties.call("GetAll", &(PLAYER,)).await?;
        let status = values
            .remove("PlaybackStatus")
            .and_then(|v| String::try_from(v).ok());
        let metadata = values
            .remove("Metadata")
            .and_then(|v| Properties::try_from(v).ok());
        let (Some(status), Some(metadata)) = (status, metadata) else {
            return Err(zbus::Error::Failure(
                "incomplete MPRIS playback information".into(),
            ));
        };
        let boolean = |name: &str| {
            values
                .get(name)
                .and_then(|v| bool::try_from(v).ok())
                .unwrap_or(false)
        };
        Ok(Self {
            status,
            can_control: boolean("CanControl"),
            can_pause: boolean("CanPause"),
            can_play: boolean("CanPlay"),
            track: Track::from_metadata(&metadata),
        })
    }
}

struct PausedPlayer {
    player: Proxy<'static>,
    properties: Proxy<'static>,
    track: Track,
    watcher: Watcher,
}

impl PausedPlayer {
    async fn prepare(connection: &Connection, name: &str) -> zbus::Result<Option<Self>> {
        // Bind to the unique owner, never a reusable application bus name. A
        // closed/reopened player must not inherit another instance's resume.
        let bus = zbus::fdo::DBusProxy::new(connection).await?;
        let owner = bus.get_name_owner(name.try_into()?).await?;
        let proxy = |interface| {
            Builder::<Proxy<'static>>::new(connection)
                .destination(owner.clone())
                .unwrap()
                .path(PATH)
                .unwrap()
                .interface(interface)
                .unwrap()
                .cache_properties(CacheProperties::No)
                .build()
        };
        let player = proxy(PLAYER).await?;
        let properties = proxy(PROPERTIES).await?;
        // Subscribe before reading the snapshot or sending Pause, avoiding a
        // gap where a track change or user playback action would be missed.
        let changes = properties.receive_signal("PropertiesChanged").await?;
        let seeks = player.receive_signal("Seeked").await?;
        let snapshot = Snapshot::read(&properties).await?;
        if snapshot.status != "Playing" || !snapshot.can_control || !snapshot.can_pause {
            return Ok(None);
        }
        let watcher = Watcher::new(changes, seeks, snapshot.track.clone());
        Ok(Some(Self {
            player,
            properties,
            track: snapshot.track,
            watcher,
        }))
    }

    async fn resume(&mut self) -> zbus::Result<()> {
        let snapshot = Snapshot::read(&self.properties).await?;
        let valid = self.watcher.finish().await;
        if valid
            && snapshot.status == "Paused"
            && snapshot.can_control
            && snapshot.can_play
            && snapshot.track == self.track
        {
            self.player.call::<_, _, ()>("Play", &()).await?;
            eprintln!("[media] resumed a player paused for dictation");
        }
        Ok(())
    }
}

struct Watcher {
    stop: Option<oneshot::Sender<()>>,
    task: JoinHandle<bool>,
}

impl Watcher {
    fn new(
        mut changes: SignalStream<'static>,
        mut seeks: SignalStream<'static>,
        track: Track,
    ) -> Self {
        let (stop, mut stopped) = oneshot::channel();
        let task = tokio::spawn(async move {
            loop {
                tokio::select! {
                    // Drain queued changes before honoring stop. GetAll's reply
                    // is ordered after earlier player signals on this connection.
                    biased;
                    message = changes.next() => {
                        let Some(message) = message else { return false };
                        let Ok((interface, mut changed, invalidated)) = message.body()
                            .deserialize::<(String, Properties, Vec<String>)>() else { return false };
                        if interface != PLAYER { continue; }
                        if invalidated.iter().any(|name| name == "PlaybackStatus" || name == "Metadata") {
                            return false;
                        }
                        if let Some(status) = changed.remove("PlaybackStatus") {
                            if <&str>::try_from(&status).ok() != Some("Paused") { return false; }
                        }
                        if let Some(metadata) = changed.remove("Metadata") {
                            let Ok(metadata) = Properties::try_from(metadata) else { return false };
                            if Track::from_metadata(&metadata) != track { return false; }
                        }
                    }
                    _ = seeks.next() => return false,
                    _ = &mut stopped => return true,
                }
            }
        });
        Self {
            stop: Some(stop),
            task,
        }
    }

    async fn finish(&mut self) -> bool {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        (&mut self.task).await.unwrap_or(false)
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[cfg(test)]
#[path = "media_playback_tests.rs"]
mod tests;
