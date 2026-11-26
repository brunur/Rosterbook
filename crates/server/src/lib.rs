//! RosterBook server.
//!
//! Serves the web UI and character data to this PC and to devices on the home
//! network. Security model:
//! - Requests from this PC (loopback) are the owner and can change setup.
//! - Other devices must pair first, using a short-lived code shown on this PC
//!   (scanned as a QR code). Pairing issues a random token stored as a cookie;
//!   only its SHA-256 hash is kept in settings, and devices can be revoked.
//! - Paired devices can view data and change view preferences (order, filters,
//!   theme), never setup, paths or devices.
//! - Every state-changing request needs an `X-Alt-Ledger: 1` header, which a
//!   web page on another site can't send without a CORS preflight we never
//!   approve (blocks cross-site request forgery).
//! - Requests addressed to an unexpected host name are refused (blocks DNS
//!   rebinding attacks that try to reach this server through a malicious site).

// Handlers return `Result<_, Response>` so a check can bail out with a ready
// HTTP error; the size of `Response` doesn't matter on this path.
#![allow(clippy::result_large_err)]

mod icons;
pub mod net;
mod queue;
mod realm_status;
mod watch;

use alt_ledger_core::{addon, ah, detect, disenchant, model, opportunities, profit, settings::{Device, GameConfig, ViewPrefs}, GameData, Settings};
use axum::{
    body::Body,
    extract::{ConnectInfo, Path as UrlPath, Query, Request, State},
    http::{header, HeaderMap, HeaderValue, Method, StatusCode},
    middleware::{self, Next},
    response::{sse::{Event, KeepAlive, Sse}, IntoResponse, Redirect, Response},
    routing::{get, post, put},
    Extension, Json, Router,
};
use futures_util::stream::{self, Stream, StreamExt};
use rust_embed::RustEmbed;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    convert::Infallible,
    net::SocketAddr,
    path::PathBuf,
    sync::{atomic::{AtomicU16, AtomicU64, Ordering}, Arc},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio::sync::{broadcast, Mutex, RwLock};
use tokio_stream::wrappers::BroadcastStream;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
const COOKIE: &str = "al_token";
const PAIR_CODE_TTL: Duration = Duration::from_secs(5 * 60);
const PAIR_MAX_ATTEMPTS: u32 = 10;

#[derive(RustEmbed)]
#[folder = "../../ui/"]
struct Ui;

pub fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

pub struct Options {
    pub settings_path: PathBuf,
    /// Serve the UI from this folder instead of the copy built into the binary (development).
    pub ui_dir: Option<PathBuf>,
    /// Override the port from settings.
    pub port: Option<u16>,
    /// Where to keep rebuildable files such as item pictures (defaults to the
    /// user's cache folder).
    pub cache_dir: Option<PathBuf>,
    /// Where `alt-ledger-queue-watcher` writes its samples and status
    /// (defaults to the same settings folder's `queue` subfolder).
    pub queue_dir: Option<PathBuf>,
    /// Where AH price history is appended to (defaults to the same settings
    /// folder's `price-history` subfolder).
    pub history_dir: Option<PathBuf>,
}

struct Pairing {
    code: String,
    expires: Instant,
    attempts: u32,
}

/// Something on this PC the owner should probably act on.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct Suggestion {
    /// "game": has RosterBook data but isn't shown. "addon": a game folder
    /// without the add-on. "addon-update": the add-on there is out of date.
    kind: &'static str,
    id: String,
    name: String,
    last_saved: Option<i64>,
    game_version: Option<String>,
    #[serde(skip)]
    config: Option<GameConfig>,
    #[serde(skip)]
    flavor_dir: PathBuf,
}

/// How often to look for newly saved game data on this PC.
const SCAN_EVERY: Duration = Duration::from_secs(120);

pub struct Inner {
    settings: RwLock<Settings>,
    settings_path: PathBuf,
    games: RwLock<HashMap<String, GameData>>,
    version: AtomicU64,
    events: broadcast::Sender<u64>,
    pairing: Mutex<Option<Pairing>>,
    watcher: Mutex<Option<watch::Watcher>>,
    ui_dir: Option<PathBuf>,
    port: AtomicU16,
    icons: icons::IconCache,
    suggestions: RwLock<Vec<Suggestion>>,
    queue_dir: PathBuf,
    history_dir: PathBuf,
    /// Last time each game's prices were captured to history, so a burst of
    /// Auctionator saves (scanning the AH writes its SavedVariables every
    /// few seconds) doesn't turn into one snapshot per write — see
    /// `capture_prices`.
    last_capture: Mutex<HashMap<String, i64>>,
    realm_status: realm_status::RealmStatus,
}

/// History only needs to resolve a multi-day trend, so there's nothing to
/// gain from capturing more often than this — and real cost (file size, load
/// time) from doing so when an active AH scan saves every few seconds.
const MIN_CAPTURE_INTERVAL: i64 = 15 * 60;

pub type AppState = Arc<Inner>;

impl Inner {
    async fn save_settings(&self) -> anyhow::Result<()> {
        let s = self.settings.read().await.clone();
        let path = self.settings_path.clone();
        tokio::task::spawn_blocking(move || s.save(&path)).await??;
        Ok(())
    }

    fn bump(&self) {
        let v = self.version.fetch_add(1, Ordering::SeqCst) + 1;
        let _ = self.events.send(v);
    }

    /// Re-reads one game's data from disk.
    pub async fn reload_game(self: &Arc<Self>, id: &str) {
        let (dir, names) = {
            let s = self.settings.read().await;
            let Some(g) = s.games.iter().find(|g| g.id == id) else { return };
            (g.saved_variables_dir.clone(), s.faction_names.clone())
        };
        let mut data = tokio::task::spawn_blocking(move || model::load_game_data(&dir, now()))
            .await
            .unwrap_or_default();
        for c in &mut data.characters {
            for r in &mut c.reputations {
                if let Some(n) = names.get(&r.id.to_string()).filter(|n| !n.is_empty()) {
                    r.name = n.clone();
                }
            }
        }
        self.games.write().await.insert(id.to_string(), data);
        self.bump();
        self.queue_icons(id).await;
    }

    /// Snapshots one game's current AH prices into its price history. A
    /// missing/empty Auctionator export isn't a failure — just nothing to
    /// capture yet — so errors are logged, not propagated.
    pub async fn capture_prices(self: &Arc<Self>, id: &str) {
        let ts = now();
        {
            let mut last = self.last_capture.lock().await;
            if last.get(id).is_some_and(|&t| ts - t < MIN_CAPTURE_INTERVAL) {
                return;
            }
            last.insert(id.to_string(), ts);
        }
        let Some(sv_dir) = self.settings.read().await.games.iter().find(|g| g.id == id).map(|g| g.saved_variables_dir.clone()) else {
            return;
        };
        let mut prices_by_realm = load_prices(sv_dir).await;
        // Bound history to items actually ranked in Opportunities, not every
        // item the AH add-on has ever cached a price for (the whole AH) —
        // see `opportunities::owned_item_ids`.
        if let Some(game) = self.games.read().await.get(id) {
            let owned = opportunities::owned_item_ids(game);
            for table in prices_by_realm.values_mut() {
                table.retain(|item_id, _| owned.contains(item_id));
            }
        }
        let (history_dir, id) = (self.history_dir.clone(), id.to_string());
        let result =
            tokio::task::spawn_blocking(move || ah::history::capture(&history_dir, &id, &prices_by_realm, ts)).await;
        match result {
            Ok(Err(e)) => eprintln!("Couldn't save price history: {e}"),
            Ok(Ok(())) => self.bump(),
            Err(_) => {}
        }
    }

    /// If item pictures are on, fetches any this game now needs, in the background.
    async fn queue_icons(self: &Arc<Self>, id: &str) {
        let flavor_dir = {
            let s = self.settings.read().await;
            if !s.game_icons {
                return;
            }
            let Some(g) = s.games.iter().find(|g| g.id == id) else { return };
            g.flavor_dir.clone()
        };
        let wanted = self.games.read().await.get(id).map(GameData::icon_ids).unwrap_or_default();
        let (state, id) = (Arc::clone(self), id.to_string());
        tokio::spawn(async move {
            state.icons.refresh(&id, flavor_dir, wanted).await;
            state.bump();
        });
    }

    pub async fn reload_all(self: &Arc<Self>) {
        let ids: Vec<String> = self.settings.read().await.games.iter().map(|g| g.id.clone()).collect();
        self.games.write().await.retain(|k, _| ids.contains(k));
        self.icons.forget_others(&ids);
        for id in ids {
            self.reload_game(&id).await;
            self.capture_prices(&id).await;
        }
        self.rewatch().await;
    }

    async fn rewatch(self: &Arc<Self>) {
        let dirs: Vec<(String, PathBuf)> =
            self.settings.read().await.games.iter().map(|g| (g.id.clone(), g.saved_variables_dir.clone())).collect();
        let w = watch::Watcher::start(Arc::clone(self), dirs);
        if let Err(e) = &w {
            eprintln!("File watching unavailable: {e}");
        }
        *self.watcher.lock().await = w.ok();
    }

    /// Looks for accounts with RosterBook data that aren't shown or dismissed.
    async fn scan_suggestions(&self) {
        let (extra, shown, dismissed) = {
            let s = self.settings.read().await;
            (s.extra_install_paths.clone(), s.games.iter().map(|g| g.id.clone()).collect::<Vec<_>>(), s.dismissed_games.clone())
        };
        let installs = tokio::task::spawn_blocking(move || detect::detect(&extra)).await.unwrap_or_default();
        let mut found = vec![];
        let shown_flavors: Vec<PathBuf> = {
            let s = self.settings.read().await;
            s.games.iter().filter_map(|g| g.flavor_dir.clone()).collect()
        };
        for flavor in installs.iter().flat_map(|i| &i.flavors) {
            // Add-on missing (in a game that's been played or is newly
            // installed) or out of date. Ignoring it lasts until the next game version.
            let version = flavor.game_version.clone().unwrap_or_default();
            let addon_id = format!("addon:{}:{version}", detect::game_id(&flavor.path));
            let kind = if flavor.addon_outdated {
                Some("addon-update")
            } else if flavor.addon_version.is_none() && (shown_flavors.contains(&flavor.path) || !flavor.accounts.is_empty() || flavor.game_version.is_some()) {
                Some("addon")
            } else {
                None
            };
            if let Some(kind) = kind.filter(|_| !dismissed.contains(&addon_id)) {
                found.push(Suggestion {
                    kind,
                    id: addon_id,
                    name: flavor.name.clone(),
                    last_saved: None,
                    game_version: flavor.game_version.clone(),
                    config: None,
                    flavor_dir: flavor.path.clone(),
                });
            }

            let with_data = flavor.accounts.iter().filter(|a| a.has_data).count();
            for a in flavor.accounts.iter().filter(|a| a.has_data) {
                if shown.contains(&a.game_id) || dismissed.contains(&a.game_id) {
                    continue;
                }
                let name = if with_data > 1 { format!("{} ({})", flavor.name, a.name) } else { flavor.name.clone() };
                found.push(Suggestion {
                    kind: "game",
                    id: a.game_id.clone(),
                    name: name.clone(),
                    last_saved: a.last_saved,
                    game_version: flavor.game_version.clone(),
                    config: Some(GameConfig {
                        id: a.game_id.clone(),
                        name,
                        saved_variables_dir: a.saved_variables_dir.clone(),
                        flavor_dir: Some(flavor.path.clone()),
                    }),
                    flavor_dir: flavor.path.clone(),
                });
            }
        }
        // Add-on fixes first (they block new data), then the most recently saved games.
        found.sort_by(|a, b| (a.kind == "game").cmp(&(b.kind == "game")).then(b.last_saved.cmp(&a.last_saved)));
        let changed = {
            let mut cur = self.suggestions.write().await;
            let changed = *cur != found;
            *cur = found;
            changed
        };
        if changed {
            self.bump();
        }
    }

    pub fn port(&self) -> u16 {
        self.port.load(Ordering::SeqCst)
    }
}

// ---------------------------------------------------------------------------
// Who is asking
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
enum Role {
    Owner,
    Device(String),
    Unpaired,
}

fn hash_token(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

fn random_hex(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    getrandom::fill(&mut buf).expect("system random number generator");
    hex::encode(buf)
}

fn cookie_token(headers: &HeaderMap) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|kv| kv.trim().split_once('='))
        .find(|(k, _)| *k == COOKIE)
        .map(|(_, v)| v.to_string())
}

/// Only plain IP addresses and `localhost` are valid host names for this server.
fn host_allowed(headers: &HeaderMap) -> bool {
    let Some(host) = headers.get(header::HOST).and_then(|h| h.to_str().ok()) else { return true };
    let name = if let Some(rest) = host.strip_prefix('[') {
        rest.split(']').next().unwrap_or("")
    } else {
        host.rsplit_once(':').map(|(h, _)| h).unwrap_or(host)
    };
    name.eq_ignore_ascii_case("localhost") || name.parse::<std::net::IpAddr>().is_ok()
}

async fn guard(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    mut req: Request,
    next: Next,
) -> Response {
    let headers = req.headers();
    if !host_allowed(headers) {
        return (StatusCode::MISDIRECTED_REQUEST, "Unknown host name").into_response();
    }
    let mutating = !matches!(*req.method(), Method::GET | Method::HEAD);
    if mutating && headers.get("x-alt-ledger").map(|v| v.as_bytes()) != Some(b"1") {
        return (StatusCode::FORBIDDEN, "Missing X-Alt-Ledger header").into_response();
    }

    let role = if peer.ip().is_loopback() {
        Role::Owner
    } else {
        let settings = state.settings.read().await;
        if !settings.lan {
            return (StatusCode::FORBIDDEN, "Network access is turned off in RosterBook's settings").into_response();
        }
        match cookie_token(headers) {
            Some(token) => {
                let h = hash_token(&token);
                match settings.devices.iter().find(|d| d.token_hash == h) {
                    Some(d) => Role::Device(d.id.clone()),
                    None => Role::Unpaired,
                }
            }
            None => Role::Unpaired,
        }
    };

    // Note when a device was last seen (at most once a minute, to avoid churn).
    if let Role::Device(id) = &role {
        let stale = state.settings.read().await.devices.iter().any(|d| &d.id == id && now() - d.last_seen > 60);
        if stale {
            {
                let mut s = state.settings.write().await;
                if let Some(d) = s.devices.iter_mut().find(|d| &d.id == id) {
                    d.last_seen = now();
                }
            }
            let _ = state.save_settings().await;
        }
    }

    req.extensions_mut().insert(role);
    next.run(req).await
}

fn need_view(role: &Role) -> Result<(), Response> {
    match role {
        Role::Unpaired => Err((StatusCode::UNAUTHORIZED, Json(json!({ "error": "pair-required" }))).into_response()),
        _ => Ok(()),
    }
}

fn need_owner(role: &Role) -> Result<(), Response> {
    match role {
        Role::Owner => Ok(()),
        _ => Err((StatusCode::FORBIDDEN, Json(json!({ "error": "owner-only" }))).into_response()),
    }
}

fn err(status: StatusCode, msg: impl Into<String>) -> Response {
    (status, Json(json!({ "error": msg.into() }))).into_response()
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

async fn session(State(s): State<AppState>, Extension(role): Extension<Role>) -> Json<Value> {
    let settings = s.settings.read().await;
    let device_name = match &role {
        Role::Device(id) => settings.devices.iter().find(|d| &d.id == id).map(|d| d.name.clone()),
        _ => None,
    };
    Json(json!({
        "role": match role { Role::Owner => "owner", Role::Device(_) => "device", Role::Unpaired => "unpaired" },
        "deviceName": device_name,
        "theme": settings.theme,
        "configured": !settings.games.is_empty(),
        "version": VERSION,
        "addonVersion": addon::bundled_version(),
    }))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GameOut<'a> {
    id: &'a str,
    name: &'a str,
    view: ViewPrefs,
    /// Item icon IDs with a picture available (only when pictures are on).
    icons: Option<Vec<u32>>,
    #[serde(flatten)]
    data: &'a GameData,
}

async fn data(State(s): State<AppState>, Extension(role): Extension<Role>) -> Response {
    if let Err(r) = need_view(&role) {
        return r;
    }
    let settings = s.settings.read().await;
    let games = s.games.read().await;
    let empty = GameData::default();
    let out: Vec<GameOut> = settings
        .games
        .iter()
        .map(|g| {
            let data = games.get(&g.id).unwrap_or(&empty);
            let icons = settings.game_icons.then(|| {
                let ready = s.icons.status(&g.id).ready;
                data.icon_ids().into_iter().filter(|i| ready.contains(i)).collect()
            });
            GameOut { id: &g.id, name: &g.name, view: settings.view(&g.id), icons, data }
        })
        .collect();
    let suggestions = if role == Role::Owner { s.suggestions.read().await.clone() } else { vec![] };
    Json(json!({ "version": s.version.load(Ordering::SeqCst), "theme": settings.theme, "games": out, "suggestions": suggestions }))
        .into_response()
}

/// The watcher's own output, as last written to disk — status and the most
/// recent samples. Not pushed over `/api/events`; the UI tab polls this
/// while it's open (the watcher writes it at most once every few seconds).
async fn get_queue(State(s): State<AppState>, Extension(role): Extension<Role>) -> Response {
    if let Err(r) = need_view(&role) {
        return r;
    }
    let status = queue::read_status(&s.queue_dir);
    let samples = queue::read_recent_samples(&s.queue_dir, 500);
    Json(json!({ "status": status, "samples": samples })).into_response()
}

/// TCP reachability of Blizzard's regional login servers, refreshed in the
/// background every minute. One server per region covers every flavor, so
/// this can't tell retail/classic/classic-era apart.
async fn get_realm_status(State(s): State<AppState>, Extension(role): Extension<Role>) -> Response {
    if let Err(r) = need_view(&role) {
        return r;
    }
    Json(s.realm_status.snapshot()).into_response()
}

async fn events(State(s): State<AppState>, Extension(role): Extension<Role>) -> Response {
    if let Err(r) = need_view(&role) {
        return r;
    }
    let hello = stream::once(async move { Ok::<_, Infallible>(Event::default().event("hello").data("0")) });
    let updates = BroadcastStream::new(s.events.subscribe())
        .filter_map(|v| async move { v.ok().map(|v| Ok::<_, Infallible>(Event::default().event("update").data(v.to_string()))) });
    let stream: std::pin::Pin<Box<dyn Stream<Item = Result<Event, Infallible>> + Send>> = Box::pin(hello.chain(updates));
    Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(20))).into_response()
}

async fn put_view(
    State(s): State<AppState>,
    Extension(role): Extension<Role>,
    UrlPath(game): UrlPath<String>,
    Json(view): Json<ViewPrefs>,
) -> Response {
    if let Err(r) = need_view(&role) {
        return r;
    }
    {
        let mut settings = s.settings.write().await;
        if !settings.games.iter().any(|g| g.id == game) {
            return err(StatusCode::NOT_FOUND, "Unknown game");
        }
        let allowed = ["custom", "level", "name", "gold", "recent"];
        let mut view = view;
        if !allowed.contains(&view.sort.as_str()) {
            view.sort = "custom".into();
        }
        settings.views.insert(game, view);
    }
    if let Err(e) = s.save_settings().await {
        return err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string());
    }
    s.bump();
    StatusCode::NO_CONTENT.into_response()
}

#[derive(Deserialize)]
struct ThemeBody {
    theme: String,
}

async fn put_theme(State(s): State<AppState>, Extension(role): Extension<Role>, Json(body): Json<ThemeBody>) -> Response {
    if let Err(r) = need_view(&role) {
        return r;
    }
    if !["system", "light", "dark"].contains(&body.theme.as_str()) {
        return err(StatusCode::BAD_REQUEST, "theme must be system, light or dark");
    }
    s.settings.write().await.theme = body.theme;
    if let Err(e) = s.save_settings().await {
        return err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string());
    }
    s.bump();
    StatusCode::NO_CONTENT.into_response()
}

// ----- setup (owner only) -----

async fn get_setup(State(s): State<AppState>, Extension(role): Extension<Role>) -> Response {
    if let Err(r) = need_owner(&role) {
        return r;
    }
    let (extra, games, lan, factions, game_icons) = {
        let st = s.settings.read().await;
        (st.extra_install_paths.clone(), st.games.clone(), st.lan, st.faction_names.clone(), st.game_icons)
    };
    let icon_status: HashMap<&str, Value> = games
        .iter()
        .map(|g| {
            let st = s.icons.status(&g.id);
            (g.id.as_str(), json!({ "count": st.ready.len(), "version": st.version, "error": st.error, "working": st.working }))
        })
        .collect();
    let installs = tokio::task::spawn_blocking(move || detect::detect(&extra)).await.unwrap_or_default();
    Json(json!({
        "installs": installs,
        "games": games,
        "lan": lan,
        "port": s.port(),
        "factionNames": factions,
        "gameIcons": game_icons,
        "iconStatus": icon_status,
        "bundledAddon": addon::bundled_version(),
        "settingsPath": s.settings_path,
    }))
    .into_response()
}

async fn put_games(State(s): State<AppState>, Extension(role): Extension<Role>, Json(mut games): Json<Vec<GameConfig>>) -> Response {
    if let Err(r) = need_owner(&role) {
        return r;
    }
    for g in &mut games {
        if !g.saved_variables_dir.is_dir() {
            return err(StatusCode::BAD_REQUEST, format!("Folder not found: {}", g.saved_variables_dir.display()));
        }
        g.id = detect::game_id(&g.saved_variables_dir);
        g.name = g.name.trim().chars().take(40).collect();
        if g.name.is_empty() {
            g.name = "WoW".into();
        }
    }
    s.settings.write().await.games = games;
    if let Err(e) = s.save_settings().await {
        return err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string());
    }
    s.reload_all().await;
    s.scan_suggestions().await;
    StatusCode::NO_CONTENT.into_response()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct InstallBody {
    flavor_dir: PathBuf,
}

async fn install_addon(Extension(role): Extension<Role>, Json(body): Json<InstallBody>) -> Response {
    if let Err(r) = need_owner(&role) {
        return r;
    }
    let dir = body.flavor_dir.clone();
    match tokio::task::spawn_blocking(move || addon::install(&dir)).await {
        Ok(Ok(version)) => {
            let dir = body.flavor_dir.clone();
            let _ = tokio::task::spawn_blocking(move || addon::install_disenchant(&dir)).await;
            Json(json!({ "installed": version })).into_response()
        }
        Ok(Err(e)) => err(StatusCode::BAD_REQUEST, e.to_string()),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

#[derive(Deserialize)]
struct PathBody {
    path: PathBuf,
}

async fn add_install_path(State(s): State<AppState>, Extension(role): Extension<Role>, Json(body): Json<PathBody>) -> Response {
    if let Err(r) = need_owner(&role) {
        return r;
    }
    if !body.path.is_dir() {
        return err(StatusCode::BAD_REQUEST, format!("Folder not found: {}", body.path.display()));
    }
    {
        let mut st = s.settings.write().await;
        if !st.extra_install_paths.contains(&body.path) {
            st.extra_install_paths.push(body.path);
        }
    }
    if let Err(e) = s.save_settings().await {
        return err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string());
    }
    s.scan_suggestions().await;
    StatusCode::NO_CONTENT.into_response()
}

/// Acts on a suggestion: `add` shows a game, `install` installs or updates the
/// add-on, `dismiss` stops suggesting it.
async fn answer_suggestion(
    State(s): State<AppState>,
    Extension(role): Extension<Role>,
    UrlPath((id, answer)): UrlPath<(String, String)>,
) -> Response {
    if let Err(r) = need_owner(&role) {
        return r;
    }
    let Some(sug) = s.suggestions.read().await.iter().find(|x| x.id == id).cloned() else {
        return err(StatusCode::NOT_FOUND, "That's already been dealt with");
    };
    if answer == "install" {
        if sug.kind == "game" {
            return err(StatusCode::BAD_REQUEST, "Nothing to install");
        }
        let dir = sug.flavor_dir.clone();
        match tokio::task::spawn_blocking(move || addon::install(&dir)).await {
            Ok(Ok(_)) => {
                let dir = sug.flavor_dir.clone();
                let _ = tokio::task::spawn_blocking(move || addon::install_disenchant(&dir)).await;
            }
            Ok(Err(e)) => return err(StatusCode::BAD_REQUEST, e.to_string()),
            Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
        }
        s.scan_suggestions().await;
        s.bump();
        return StatusCode::NO_CONTENT.into_response();
    }
    {
        let mut st = s.settings.write().await;
        match answer.as_str() {
            "add" => {
                let Some(config) = sug.config else { return err(StatusCode::BAD_REQUEST, "Not a game") };
                if !st.games.iter().any(|g| g.id == id) {
                    st.games.push(config);
                }
            }
            "dismiss" => {
                if !st.dismissed_games.contains(&id) {
                    st.dismissed_games.push(id);
                }
            }
            _ => return err(StatusCode::NOT_FOUND, "Unknown answer"),
        }
    }
    if let Err(e) = s.save_settings().await {
        return err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string());
    }
    if answer == "add" {
        s.reload_all().await;
    }
    s.scan_suggestions().await;
    s.bump();
    StatusCode::NO_CONTENT.into_response()
}

#[derive(Deserialize)]
struct IconsBody {
    enabled: bool,
}

async fn put_icons(State(s): State<AppState>, Extension(role): Extension<Role>, Json(body): Json<IconsBody>) -> Response {
    if let Err(r) = need_owner(&role) {
        return r;
    }
    s.settings.write().await.game_icons = body.enabled;
    if let Err(e) = s.save_settings().await {
        return err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string());
    }
    let ids: Vec<String> = s.settings.read().await.games.iter().map(|g| g.id.clone()).collect();
    for id in ids {
        s.queue_icons(&id).await;
    }
    s.bump();
    StatusCode::NO_CONTENT.into_response()
}

/// Every supported price add-on's data in one account's SavedVariables,
/// folded together by realm (a later source overwrites an earlier one for
/// the same item — moot today with only one source, ready for more).
async fn load_prices(sv_dir: PathBuf) -> HashMap<String, ah::PriceTable> {
    tokio::task::spawn_blocking(move || {
        ah::Source::ALL.iter().fold(HashMap::new(), |mut acc, source| {
            for (realm, table) in ah::load_all(&sv_dir, *source) {
                acc.entry(realm).or_insert_with(HashMap::new).extend(table);
            }
            acc
        })
    })
    .await
    .unwrap_or_default()
}

/// Known recipes ranked by AH profit, for one game. Reads whichever
/// supported price add-on has data in that account's SavedVariables; an
/// account with none shows an empty list rather than an error.
async fn get_profit(State(s): State<AppState>, Extension(role): Extension<Role>, UrlPath(game): UrlPath<String>) -> Response {
    if let Err(r) = need_view(&role) {
        return r;
    }
    let Some(sv_dir) = s.settings.read().await.games.iter().find(|g| g.id == game).map(|g| g.saved_variables_dir.clone()) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Some(data) = s.games.read().await.get(&game).cloned() else {
        return Json(json!([])).into_response();
    };
    let prices_by_realm = load_prices(sv_dir).await;
    Json(profit::rank_recipes(&data, &prices_by_realm)).into_response()
}

/// What to do with items already sitting in bags/bank — vendor, AH, or
/// disenchant — ranked by best known value. Reads the disenchant add-on's
/// learned yields the same way `get_profit` reads AH prices: an account
/// with none shows an empty list rather than an error.
async fn get_opportunities(State(s): State<AppState>, Extension(role): Extension<Role>, UrlPath(game): UrlPath<String>) -> Response {
    if let Err(r) = need_view(&role) {
        return r;
    }
    let Some(sv_dir) = s.settings.read().await.games.iter().find(|g| g.id == game).map(|g| g.saved_variables_dir.clone()) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Some(data) = s.games.read().await.get(&game).cloned() else {
        return Json(json!([])).into_response();
    };
    let prices_by_realm = load_prices(sv_dir.clone()).await;
    let disenchant_data =
        tokio::task::spawn_blocking(move || disenchant::load_disenchant_data(&sv_dir)).await.unwrap_or_default();
    let history_dir = s.history_dir.clone();
    let history = tokio::task::spawn_blocking(move || ah::history::load(&history_dir, &game)).await.unwrap_or_default();
    Json(opportunities::rank_disposals(&data, &disenchant_data, &prices_by_realm, &history, now())).into_response()
}

/// A cached item picture. Paths come only from the cache, never from the request.
async fn icon(State(s): State<AppState>, Extension(role): Extension<Role>, UrlPath((game, id)): UrlPath<(String, u32)>) -> Response {
    if let Err(r) = need_view(&role) {
        return r;
    }
    if !s.settings.read().await.game_icons {
        return StatusCode::NOT_FOUND.into_response();
    }
    let Some(path) = s.icons.file(&game, id) else { return StatusCode::NOT_FOUND.into_response() };
    match tokio::fs::read(path).await {
        Ok(bytes) => Response::builder()
            .header(header::CONTENT_TYPE, "image/png")
            .header(header::CACHE_CONTROL, "private, max-age=86400")
            .header("X-Content-Type-Options", "nosniff")
            .body(Body::from(bytes))
            .unwrap(),
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

#[derive(Deserialize)]
struct LanBody {
    lan: bool,
}

async fn put_lan(State(s): State<AppState>, Extension(role): Extension<Role>, Json(body): Json<LanBody>) -> Response {
    if let Err(r) = need_owner(&role) {
        return r;
    }
    s.settings.write().await.lan = body.lan;
    match s.save_settings().await {
        Ok(()) => {
            s.bump();
            StatusCode::NO_CONTENT.into_response()
        }
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn put_faction_names(
    State(s): State<AppState>,
    Extension(role): Extension<Role>,
    Json(names): Json<HashMap<String, String>>,
) -> Response {
    if let Err(r) = need_owner(&role) {
        return r;
    }
    s.settings.write().await.faction_names = names.into_iter().filter(|(_, v)| !v.trim().is_empty()).collect();
    if let Err(e) = s.save_settings().await {
        return err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string());
    }
    s.reload_all().await;
    StatusCode::NO_CONTENT.into_response()
}

// ----- pairing & devices -----

async fn start_pairing(State(s): State<AppState>, Extension(role): Extension<Role>) -> Response {
    if let Err(r) = need_owner(&role) {
        return r;
    }
    let mut n = [0u8; 4];
    getrandom::fill(&mut n).expect("system random number generator");
    let code = format!("{:06}", u32::from_le_bytes(n) % 1_000_000);
    *s.pairing.lock().await = Some(Pairing { code: code.clone(), expires: Instant::now() + PAIR_CODE_TTL, attempts: 0 });

    let port = s.port();
    let lan_on = s.settings.read().await.lan;
    let urls: Vec<Value> = net::lan_addresses()
        .into_iter()
        .map(|a| {
            let url = format!("http://{}:{}/pair?code={}", a.ip, port, code);
            json!({ "adapter": a.adapter, "likely": a.likely, "base": format!("http://{}:{}", a.ip, port), "qr": net::qr_svg(&url), "url": url })
        })
        .collect();
    Json(json!({ "code": code, "expiresIn": PAIR_CODE_TTL.as_secs(), "lan": lan_on, "port": port, "urls": urls })).into_response()
}

#[derive(Deserialize)]
struct PairQuery {
    code: Option<String>,
}

fn device_name_from(headers: &HeaderMap) -> String {
    let ua = headers.get(header::USER_AGENT).and_then(|v| v.to_str().ok()).unwrap_or("").to_lowercase();
    let kind = if ua.contains("ipad") || (ua.contains("android") && !ua.contains("mobile")) || ua.contains("tablet") {
        "Tablet"
    } else if ua.contains("iphone") || ua.contains("android") || ua.contains("mobile") {
        "Phone"
    } else {
        "Computer"
    };
    let os = if ua.contains("ipad") || ua.contains("iphone") { " (iOS)" }
        else if ua.contains("android") { " (Android)" }
        else if ua.contains("windows") { " (Windows)" }
        else if ua.contains("mac os") { " (Mac)" }
        else if ua.contains("linux") { " (Linux)" }
        else { "" };
    format!("{kind}{os}")
}

/// Visited from the QR code on another device. Exchanges a valid code for a device token.
async fn pair(
    State(s): State<AppState>,
    Extension(role): Extension<Role>,
    headers: HeaderMap,
    Query(q): Query<PairQuery>,
) -> Response {
    if role != Role::Unpaired {
        return Redirect::to("/").into_response();
    }
    // This is meant to be visited as a real page load (a scanned QR code, a
    // tapped link), not loaded in the background by a hostile page on the
    // same network (e.g. `<img src="http://host/pair?code=...">`) — modern
    // browsers mark that distinction in Sec-Fetch-Dest, so reject anything
    // that isn't a top-level navigation. An older browser that sends no such
    // header at all still goes through, same as before.
    if headers.get("sec-fetch-dest").is_some_and(|v| v != "document") {
        return Redirect::to("/?pair=failed").into_response();
    }
    let supplied = q.code.unwrap_or_default();
    let ok = {
        let mut guard = s.pairing.lock().await;
        match guard.as_mut() {
            Some(p) if p.expires > Instant::now() && p.attempts < PAIR_MAX_ATTEMPTS => {
                if p.code == supplied {
                    *guard = None; // single use
                    true
                } else {
                    p.attempts += 1;
                    false
                }
            }
            _ => false,
        }
    };
    if !ok {
        return Redirect::to("/?pair=failed").into_response();
    }

    let token = random_hex(32);
    let device = Device {
        id: random_hex(6),
        name: device_name_from(&headers),
        token_hash: hash_token(&token),
        created: now(),
        last_seen: now(),
    };
    s.settings.write().await.devices.push(device);
    if let Err(e) = s.save_settings().await {
        return err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string());
    }
    s.bump();

    let cookie = format!("{COOKIE}={token}; Path=/; HttpOnly; SameSite=Strict; Max-Age=31536000");
    let mut resp = Redirect::to("/?paired=1").into_response();
    if let Ok(v) = HeaderValue::from_str(&cookie) {
        resp.headers_mut().insert(header::SET_COOKIE, v);
    }
    resp
}

async fn list_devices(State(s): State<AppState>, Extension(role): Extension<Role>) -> Response {
    if let Err(r) = need_owner(&role) {
        return r;
    }
    let devices: Vec<Value> = s
        .settings
        .read()
        .await
        .devices
        .iter()
        .map(|d| json!({ "id": d.id, "name": d.name, "created": d.created, "lastSeen": d.last_seen }))
        .collect();
    Json(devices).into_response()
}

#[derive(Deserialize)]
struct RenameBody {
    name: String,
}

async fn rename_device(
    State(s): State<AppState>,
    Extension(role): Extension<Role>,
    UrlPath(id): UrlPath<String>,
    Json(body): Json<RenameBody>,
) -> Response {
    if let Err(r) = need_owner(&role) {
        return r;
    }
    {
        let mut st = s.settings.write().await;
        match st.devices.iter_mut().find(|d| d.id == id) {
            Some(d) => d.name = body.name.trim().chars().take(40).collect(),
            None => return err(StatusCode::NOT_FOUND, "Unknown device"),
        }
    }
    match s.save_settings().await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn remove_device(State(s): State<AppState>, Extension(role): Extension<Role>, UrlPath(id): UrlPath<String>) -> Response {
    if let Err(r) = need_owner(&role) {
        return r;
    }
    s.settings.write().await.devices.retain(|d| d.id != id);
    if let Err(e) = s.save_settings().await {
        return err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string());
    }
    s.bump(); // connected pages refetch and the removed device loses access
    StatusCode::NO_CONTENT.into_response()
}

// ----- static UI -----

async fn static_file(State(s): State<AppState>, req: Request) -> Response {
    let path = req.uri().path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };
    if path.contains("..") {
        return StatusCode::NOT_FOUND.into_response();
    }
    let read = |p: &str| -> Option<Vec<u8>> {
        match &s.ui_dir {
            Some(dir) => std::fs::read(dir.join(p)).ok(),
            None => Ui::get(p).map(|f| f.data.into_owned()),
        }
    };
    // Unknown paths fall back to the app shell.
    let (bytes, path) = match read(path) {
        Some(b) => (b, path.to_string()),
        None => match read("index.html") {
            Some(b) => (b, "index.html".to_string()),
            None => return StatusCode::NOT_FOUND.into_response(),
        },
    };
    let mime = mime_guess::from_path(&path).first_or_octet_stream();
    // Fonts never change; everything else must be rechecked so a new version
    // of the app is never shown with an old copy of its interface.
    let cache = if path.starts_with("fonts/") { "public, max-age=604800, immutable" } else { "no-cache" };
    Response::builder()
        .header(header::CONTENT_TYPE, mime.as_ref())
        .header(header::CACHE_CONTROL, cache)
        .header("X-Content-Type-Options", "nosniff")
        .header("Referrer-Policy", "no-referrer")
        .header(
            "Content-Security-Policy",
            "default-src 'self'; img-src 'self' data:; style-src 'self' 'unsafe-inline'; script-src 'self'; connect-src 'self'; frame-ancestors 'none'",
        )
        .body(Body::from(bytes))
        .unwrap()
}

// ---------------------------------------------------------------------------
// Start
// ---------------------------------------------------------------------------

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/session", get(session))
        .route("/api/data", get(data))
        .route("/api/queue", get(get_queue))
        .route("/api/realm-status", get(get_realm_status))
        .route("/api/events", get(events))
        .route("/api/view/{game}", put(put_view))
        .route("/api/theme", put(put_theme))
        .route("/api/setup", get(get_setup))
        .route("/api/setup/games", put(put_games))
        .route("/api/setup/install-addon", post(install_addon))
        .route("/api/setup/install-path", post(add_install_path))
        .route("/api/setup/lan", put(put_lan))
        .route("/api/setup/icons", put(put_icons))
        .route("/api/setup/suggestions/{id}/{answer}", post(answer_suggestion))
        .route("/api/icon/{game}/{id}", get(icon))
        .route("/api/profit/{game}", get(get_profit))
        .route("/api/opportunities/{game}", get(get_opportunities))
        .route("/api/setup/faction-names", put(put_faction_names))
        .route("/api/pair", post(start_pairing))
        .route("/api/devices", get(list_devices))
        .route("/api/devices/{id}", put(rename_device).delete(remove_device))
        .route("/pair", get(pair))
        .fallback(static_file)
        .layer(middleware::from_fn_with_state(state.clone(), guard))
        .with_state(state)
}

pub struct Running {
    pub state: AppState,
    pub port: u16,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
    pub task: tokio::task::JoinHandle<()>,
}

impl Running {
    pub fn local_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    pub fn stop(&mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
    }
}

/// Loads settings, starts watching game folders and serves on all interfaces.
/// If the configured port is taken, tries the next few ports.
pub async fn start(opts: Options) -> anyhow::Result<Running> {
    let settings = Settings::load(&opts.settings_path).unwrap_or_else(|e| {
        eprintln!("Couldn't read settings ({e}); starting with defaults.");
        Settings::default()
    });
    let base_port = opts.port.unwrap_or(settings.port);

    let mut listener = None;
    for port in base_port..base_port.saturating_add(10) {
        if let Ok(l) = tokio::net::TcpListener::bind(SocketAddr::from(([0, 0, 0, 0], port))).await {
            listener = Some(l);
            break;
        }
    }
    let listener = listener.ok_or_else(|| anyhow::anyhow!("ports {base_port}-{} are all in use", base_port + 9))?;
    let port = listener.local_addr()?.port();

    let (events, _) = broadcast::channel(64);
    let state: AppState = Arc::new(Inner {
        settings: RwLock::new(settings),
        settings_path: opts.settings_path,
        games: RwLock::new(HashMap::new()),
        version: AtomicU64::new(0),
        events,
        pairing: Mutex::new(None),
        watcher: Mutex::new(None),
        ui_dir: opts.ui_dir,
        port: AtomicU16::new(port),
        icons: icons::IconCache::new(&opts.cache_dir.unwrap_or_else(alt_ledger_core::settings::default_cache_dir)),
        suggestions: RwLock::new(Vec::new()),
        queue_dir: opts.queue_dir.unwrap_or_else(|| alt_ledger_core::settings::default_dir().join("queue")),
        history_dir: opts.history_dir.unwrap_or_else(|| alt_ledger_core::settings::default_dir().join("price-history")),
        last_capture: Mutex::new(HashMap::new()),
        realm_status: realm_status::RealmStatus::new(),
    });
    state.reload_all().await;

    let pinger = Arc::downgrade(&state);
    tokio::spawn(async move {
        loop {
            let Some(state) = pinger.upgrade() else { break };
            state.realm_status.refresh().await;
            drop(state);
            tokio::time::sleep(realm_status::CHECK_EVERY).await;
        }
    });

    // Notice games that start saving RosterBook data while the app is running.
    let scanner = Arc::downgrade(&state);
    tokio::spawn(async move {
        loop {
            let Some(state) = scanner.upgrade() else { break };
            state.scan_suggestions().await;
            drop(state);
            tokio::time::sleep(SCAN_EVERY).await;
        }
    });

    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    let app = router(state.clone()).into_make_service_with_connect_info::<SocketAddr>();
    let task = tokio::spawn(async move {
        let _ = axum::serve(listener, app).with_graceful_shutdown(async { let _ = rx.await; }).await;
    });

    Ok(Running { state, port, shutdown: Some(tx), task })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_header_rules() {
        let mut h = HeaderMap::new();
        for (host, ok) in [
            ("localhost:8787", true),
            ("127.0.0.1:8787", true),
            ("192.168.1.20:8787", true),
            ("[::1]:8787", true),
            ("evil.example.com", false),
            ("attacker.localtest.me:8787", false),
        ] {
            h.insert(header::HOST, HeaderValue::from_static(host));
            assert_eq!(host_allowed(&h), ok, "{host}");
        }
    }

    #[test]
    fn reads_token_cookie() {
        let mut h = HeaderMap::new();
        h.insert(header::COOKIE, HeaderValue::from_static("a=1; al_token=abc123; b=2"));
        assert_eq!(cookie_token(&h).as_deref(), Some("abc123"));
    }
}
