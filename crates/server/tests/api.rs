//! End-to-end tests against a real server on a random port.

use alt_ledger_server::{net, start, Options};
use serde_json::{json, Value};
use std::time::Duration;

const FIXTURE: &str = include_str!("../../core/tests/fixtures/AltLedger.retail.lua");

struct Setup {
    _tmp: tempfile::TempDir,
    sv_dir: std::path::PathBuf,
    queue_dir: std::path::PathBuf,
    port: u16,
    _server: alt_ledger_server::Running,
}

async fn setup() -> Setup {
    let tmp = tempfile::tempdir().unwrap();
    let sv_dir = tmp.path().join("SavedVariables");
    std::fs::create_dir_all(&sv_dir).unwrap();
    std::fs::write(sv_dir.join("AltLedger.lua"), FIXTURE).unwrap();
    let server = start(Options {
        settings_path: tmp.path().join("settings.json"),
        ui_dir: None,
        port: Some(0),
        cache_dir: Some(tmp.path().join("cache")),
        queue_dir: Some(tmp.path().join("queue")),
    })
    .await
    .unwrap();
    let queue_dir = tmp.path().join("queue");
    Setup { port: server.port, sv_dir, queue_dir, _tmp: tmp, _server: server }
}

fn client() -> reqwest::Client {
    reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).build().unwrap()
}

#[tokio::test]
async fn owner_can_configure_and_read_data() {
    let s = setup().await;
    let base = format!("http://127.0.0.1:{}", s.port);
    let c = client();

    let session: Value = c.get(format!("{base}/api/session")).send().await.unwrap().json().await.unwrap();
    assert_eq!(session["role"], "owner");
    assert_eq!(session["configured"], false);

    // Changes need the anti-CSRF header.
    let games = json!([{ "id": "", "name": "Retail", "savedVariablesDir": s.sv_dir }]);
    let r = c.put(format!("{base}/api/setup/games")).json(&games).send().await.unwrap();
    assert_eq!(r.status(), 403);
    let r = c.put(format!("{base}/api/setup/games")).header("X-Alt-Ledger", "1").json(&games).send().await.unwrap();
    assert_eq!(r.status(), 204);

    let data: Value = c.get(format!("{base}/api/data")).send().await.unwrap().json().await.unwrap();
    let game = &data["games"][0];
    assert_eq!(game["name"], "Retail");
    assert_eq!(game["characters"][0]["firstName"], "Norasonia");
    assert_eq!(game["view"]["sort"], "custom");
    let id = game["id"].as_str().unwrap().to_string();

    // View preferences round-trip.
    let view = json!({ "sort": "level", "order": ["x"], "pinned": [], "hidden": [], "filters": { "factions": ["Alliance"] } });
    let r = c.put(format!("{base}/api/view/{id}")).header("X-Alt-Ledger", "1").json(&view).send().await.unwrap();
    assert_eq!(r.status(), 204);
    let data: Value = c.get(format!("{base}/api/data")).send().await.unwrap().json().await.unwrap();
    assert_eq!(data["games"][0]["view"]["sort"], "level");
    assert_eq!(data["games"][0]["view"]["filters"]["factions"][0], "Alliance");

    // The UI is served, with security headers.
    let r = c.get(format!("{base}/")).send().await.unwrap();
    assert!(r.headers().get("content-security-policy").is_some());
}

#[tokio::test]
async fn serves_what_the_queue_watcher_wrote() {
    let s = setup().await;
    let base = format!("http://127.0.0.1:{}", s.port);
    let c = client();

    // Nothing written yet.
    let empty: Value = c.get(format!("{base}/api/queue")).send().await.unwrap().json().await.unwrap();
    assert_eq!(empty["status"], Value::Null);
    assert_eq!(empty["samples"], json!([]));

    std::fs::create_dir_all(&s.queue_dir).unwrap();
    std::fs::write(s.queue_dir.join("watcher-status.json"), r#"{"updatedAt":1000,"cadence":"active","error":null}"#)
        .unwrap();
    std::fs::write(
        s.queue_dir.join("queue-samples.jsonl"),
        r#"{"id":"1","sessionId":"s1","ts":1000,"position":42,"displayedTimeText":null,"parseOk":true}"#.to_string()
            + "\n",
    )
    .unwrap();

    let body: Value = c.get(format!("{base}/api/queue")).send().await.unwrap().json().await.unwrap();
    assert_eq!(body["status"]["cadence"], "active");
    assert_eq!(body["samples"][0]["position"], 42);
}

#[tokio::test]
async fn rejects_unknown_host_names() {
    let s = setup().await;
    let r = client()
        .get(format!("http://127.0.0.1:{}/api/data", s.port))
        .header("Host", "evil.example.com")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 421);
}

#[tokio::test]
async fn reloads_when_the_game_saves() {
    let s = setup().await;
    let base = format!("http://127.0.0.1:{}", s.port);
    let c = client();
    let games = json!([{ "id": "", "name": "Retail", "savedVariablesDir": s.sv_dir }]);
    c.put(format!("{base}/api/setup/games")).header("X-Alt-Ledger", "1").json(&games).send().await.unwrap();

    let before: Value = c.get(format!("{base}/api/data")).send().await.unwrap().json().await.unwrap();
    // Simulate WoW writing new data (Norasonia gets richer).
    std::fs::write(s.sv_dir.join("AltLedger.lua"), FIXTURE.replace("[\"money\"] = 9308", "[\"money\"] = 123456")).unwrap();
    let mut updated = false;
    for _ in 0..40 {
        tokio::time::sleep(Duration::from_millis(150)).await;
        let now: Value = c.get(format!("{base}/api/data")).send().await.unwrap().json().await.unwrap();
        if now["version"].as_u64() > before["version"].as_u64() && now["games"][0]["characters"][0]["money"] == 123456 {
            updated = true;
            break;
        }
    }
    assert!(updated, "data should reload after the file changes");
}

/// Talks to the server over the machine's network address, like a tablet would.
#[tokio::test]
async fn devices_must_pair_and_can_be_revoked() {
    let Some(lan) = net::lan_addresses().into_iter().next() else {
        eprintln!("no network interface; skipping");
        return;
    };
    let s = setup().await;
    let owner = format!("http://127.0.0.1:{}", s.port);
    let remote = format!("http://{}:{}", lan.ip, s.port);
    let c = client();
    let games = json!([{ "id": "", "name": "Retail", "savedVariablesDir": s.sv_dir }]);
    c.put(format!("{owner}/api/setup/games")).header("X-Alt-Ledger", "1").json(&games).send().await.unwrap();

    // Unpaired devices see nothing.
    let session: Value = c.get(format!("{remote}/api/session")).send().await.unwrap().json().await.unwrap();
    assert_eq!(session["role"], "unpaired");
    assert_eq!(c.get(format!("{remote}/api/data")).send().await.unwrap().status(), 401);
    assert_eq!(c.get(format!("{remote}/api/events")).send().await.unwrap().status(), 401);

    // Only the owner can start pairing.
    let r = c.post(format!("{remote}/api/pair")).header("X-Alt-Ledger", "1").send().await.unwrap();
    assert_eq!(r.status(), 403);
    let pairing: Value = c.post(format!("{owner}/api/pair")).header("X-Alt-Ledger", "1").send().await.unwrap().json().await.unwrap();
    let code = pairing["code"].as_str().unwrap().to_string();
    assert_eq!(code.len(), 6);
    assert!(pairing["urls"][0]["qr"].as_str().unwrap().contains("<svg"));

    // A wrong code fails; the right one sets a cookie once.
    let wrong = if code == "000000" { "111111" } else { "000000" };
    let r = c.get(format!("{remote}/pair?code={wrong}")).send().await.unwrap();
    assert_eq!(r.headers()["location"], "/?pair=failed");
    let r = c.get(format!("{remote}/pair?code={code}")).send().await.unwrap();
    assert_eq!(r.headers()["location"], "/?paired=1");
    let cookie = r.headers()["set-cookie"].to_str().unwrap().split(';').next().unwrap().to_string();
    assert!(cookie.starts_with("al_token="));
    let r = c.get(format!("{remote}/pair?code={code}")).send().await.unwrap();
    assert_eq!(r.headers()["location"], "/?pair=failed", "codes are single use");

    // Paired: can view and change view settings, but not setup.
    let get = |path: &str| c.get(format!("{remote}{path}")).header("cookie", &cookie).send();
    assert_eq!(get("/api/data").await.unwrap().status(), 200);
    let session: Value = get("/api/session").await.unwrap().json().await.unwrap();
    assert_eq!(session["role"], "device");
    assert_eq!(get("/api/setup").await.unwrap().status(), 403);
    assert_eq!(get("/api/devices").await.unwrap().status(), 403);
    let r = c.put(format!("{remote}/api/theme")).header("cookie", &cookie).header("X-Alt-Ledger", "1")
        .json(&json!({ "theme": "light" })).send().await.unwrap();
    assert_eq!(r.status(), 204);

    // Revoked devices lose access immediately.
    let devices: Value = c.get(format!("{owner}/api/devices")).send().await.unwrap().json().await.unwrap();
    let id = devices[0]["id"].as_str().unwrap();
    let r = c.delete(format!("{owner}/api/devices/{id}")).header("X-Alt-Ledger", "1").send().await.unwrap();
    assert_eq!(r.status(), 204);
    assert_eq!(get("/api/data").await.unwrap().status(), 401);

    // Turning network access off blocks other devices entirely.
    c.put(format!("{owner}/api/setup/lan")).header("X-Alt-Ledger", "1").json(&json!({ "lan": false })).send().await.unwrap();
    assert_eq!(c.get(format!("{remote}/api/session")).send().await.unwrap().status(), 403);
}

/// Item pictures: off by default, extracted from the install once enabled,
/// served to the owner, and skipped for icons the install doesn't have.
#[tokio::test]
async fn serves_item_pictures_from_the_install() {
    let s = setup().await;
    let base = format!("http://127.0.0.1:{}", s.port);
    let c = client();

    let ids: Vec<u32> = FIXTURE
        .split("[\"icon\"] = ")
        .skip(1)
        .filter_map(|rest| rest.split(',').next()?.trim().parse().ok())
        .collect();
    assert!(ids.len() >= 3, "fixture should have item icons");
    let (absent, present) = ids.split_first().unwrap();
    let files: Vec<(u32, Vec<u8>)> =
        present.iter().filter(|id| *id != absent).map(|&id| (id, alt_ledger_casc::testing::sample_icon())).collect();
    let install = tempfile::tempdir().unwrap();
    let flavor = alt_ledger_casc::testing::fake_install(install.path(), &files);

    let games = json!([{ "id": "", "name": "Retail", "savedVariablesDir": s.sv_dir, "flavorDir": flavor }]);
    c.put(format!("{base}/api/setup/games")).header("X-Alt-Ledger", "1").json(&games).send().await.unwrap();
    let data: Value = c.get(format!("{base}/api/data")).send().await.unwrap().json().await.unwrap();
    assert!(data["games"][0]["icons"].is_null(), "pictures are off by default");
    let game = data["games"][0]["id"].as_str().unwrap().to_string();
    let icon_url = |id: u32| format!("{base}/api/icon/{game}/{id}");
    assert_eq!(c.get(icon_url(present[0])).send().await.unwrap().status(), 404);

    let r = c.put(format!("{base}/api/setup/icons")).header("X-Alt-Ledger", "1").json(&json!({ "enabled": true })).send().await.unwrap();
    assert_eq!(r.status(), 204);
    let mut ready = vec![];
    for _ in 0..50 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        let data: Value = c.get(format!("{base}/api/data")).send().await.unwrap().json().await.unwrap();
        ready = data["games"][0]["icons"].as_array().cloned().unwrap_or_default();
        if !ready.is_empty() {
            break;
        }
    }
    let ready: Vec<u32> = ready.iter().filter_map(|v| v.as_u64()).map(|v| v as u32).collect();
    assert!(!ready.is_empty(), "pictures should be extracted");
    assert!(!ready.contains(absent), "icons missing from the install are left out");

    let r = c.get(icon_url(ready[0])).send().await.unwrap();
    assert_eq!(r.status(), 200);
    assert_eq!(r.headers()["content-type"], "image/png");
    assert_eq!(&r.bytes().await.unwrap()[1..4], b"PNG");
    assert_eq!(c.get(icon_url(*absent)).send().await.unwrap().status(), 404);

    let setup: Value = c.get(format!("{base}/api/setup")).send().await.unwrap().json().await.unwrap();
    assert_eq!(setup["gameIcons"], true);
    assert_eq!(setup["iconStatus"][&game]["version"], "1.60.1.70450");

    // Turning it off stops serving them.
    c.put(format!("{base}/api/setup/icons")).header("X-Alt-Ledger", "1").json(&json!({ "enabled": false })).send().await.unwrap();
    assert_eq!(c.get(icon_url(ready[0])).send().await.unwrap().status(), 404);
}

/// A game that starts saving Alt Ledger data is offered to the owner, once,
/// and can be added or dismissed.
#[tokio::test]
async fn offers_games_with_new_data() {
    let s = setup().await;
    let base = format!("http://127.0.0.1:{}", s.port);
    let c = client();
    let put = |path: &str, body: Value| c.put(format!("{base}{path}")).header("X-Alt-Ledger", "1").json(&body).send();
    let post = |path: &str, body: Value| c.post(format!("{base}{path}")).header("X-Alt-Ledger", "1").json(&body).send();
    let data = || async { c.get(format!("{base}/api/data")).send().await.unwrap().json::<Value>().await.unwrap() };

    // Show only the Retail test folder, then a Forever beta install appears with data.
    put("/api/setup/games", json!([{ "id": "", "name": "Retail", "savedVariablesDir": s.sv_dir }])).await.unwrap();
    let wow = tempfile::tempdir().unwrap();
    let flavor = wow.path().join("World of Warcraft/_classic_beta_");
    for account in ["ONE", "TWO"] {
        let sv = flavor.join(format!("WTF/Account/{account}/SavedVariables"));
        std::fs::create_dir_all(&sv).unwrap();
        std::fs::write(sv.join("AltLedger.lua"), FIXTURE).unwrap();
    }
    std::fs::create_dir_all(flavor.join("WTF/Account/EMPTY/SavedVariables")).unwrap();
    assert_eq!(post("/api/setup/install-path", json!({ "path": wow.path() })).await.unwrap().status(), 204);

    let games_offered = |d: &Value| -> Vec<Value> {
        d["suggestions"].as_array().unwrap().iter().filter(|s| s["kind"] == "game").cloned().collect()
    };
    let d = data().await;
    let offered = games_offered(&d);
    assert_eq!(offered.len(), 2, "both accounts with data, not the empty one: {offered:?}");
    assert!(offered.iter().all(|o| o["name"].as_str().unwrap().starts_with("Forever (")));
    assert!(offered[0].get("config").is_none(), "folder paths aren't sent");
    let (first, second) = (offered[0]["id"].as_str().unwrap().to_string(), offered[1]["id"].as_str().unwrap().to_string());

    // Add one: it becomes a game with characters and stops being offered.
    assert_eq!(post(&format!("/api/setup/suggestions/{first}/add"), json!({})).await.unwrap().status(), 204);
    let d = data().await;
    let added = d["games"].as_array().unwrap().iter().find(|g| g["id"] == first.as_str()).expect("added");
    assert!(!added["characters"].as_array().unwrap().is_empty());
    assert_eq!(games_offered(&d).len(), 1);

    // Dismiss the other: it's not offered again, even after another scan.
    assert_eq!(post(&format!("/api/setup/suggestions/{second}/dismiss"), json!({})).await.unwrap().status(), 204);
    post("/api/setup/install-path", json!({ "path": wow.path() })).await.unwrap();
    assert!(games_offered(&data().await).is_empty());
    assert_eq!(post(&format!("/api/setup/suggestions/{second}/add"), json!({})).await.unwrap().status(), 404);
}

/// Launch day: a new game folder appears (no WTF yet) at a version the add-on
/// doesn't list. The app offers to install the add-on, declaring that version,
/// and offers an update when the game patches again.
#[tokio::test]
async fn offers_to_install_and_update_the_addon() {
    let s = setup().await;
    let base = format!("http://127.0.0.1:{}", s.port);
    let c = client();
    let post = |path: &str, body: Value| c.post(format!("{base}{path}")).header("X-Alt-Ledger", "1").json(&body).send();
    let addon_offers = || async {
        let d: Value = c.get(format!("{base}/api/data")).send().await.unwrap().json().await.unwrap();
        d["suggestions"].as_array().unwrap().iter().filter(|s| s["kind"] != "game").cloned().collect::<Vec<Value>>()
    };

    let wow = tempfile::tempdir().unwrap();
    let root = wow.path().join("World of Warcraft");
    let flavor = root.join("_forever_");
    std::fs::create_dir_all(&flavor).unwrap();
    std::fs::write(flavor.join(".flavor.info"), "Product Flavor!STRING:0\nwow_forever\n").unwrap();
    let set_version = |v: &str| {
        std::fs::write(root.join(".build.info"), format!("Branch!STRING:0|Active!DEC:1|Version!STRING:0|Product!STRING:0\nus|1|{v}|wow_forever\n")).unwrap()
    };
    set_version("1.61.0.71000");
    post("/api/setup/install-path", json!({ "path": wow.path() })).await.unwrap();

    let offers = addon_offers().await;
    assert_eq!(offers.len(), 1, "{offers:?}");
    assert_eq!(offers[0]["kind"], "addon");
    assert_eq!(offers[0]["name"], "Forever");
    assert_eq!(offers[0]["gameVersion"], "1.61.0.71000");
    let id = offers[0]["id"].as_str().unwrap().to_string();
    assert_eq!(post(&format!("/api/setup/suggestions/{id}/install"), json!({})).await.unwrap().status(), 204);
    let toc = std::fs::read_to_string(flavor.join("Interface/AddOns/AltLedger/AltLedger.toc")).unwrap();
    assert!(toc.contains("16100"), "declares the installed game version: {toc}");
    assert!(addon_offers().await.is_empty());

    // The game patches.
    set_version("1.61.1.71200");
    post("/api/setup/install-path", json!({ "path": wow.path() })).await.unwrap();
    let offers = addon_offers().await;
    assert_eq!(offers.len(), 1);
    assert_eq!(offers[0]["kind"], "addon-update");
    let id = offers[0]["id"].as_str().unwrap().to_string();
    assert_eq!(post(&format!("/api/setup/suggestions/{id}/install"), json!({})).await.unwrap().status(), 204);
    assert!(addon_offers().await.is_empty());
}

#[tokio::test]
async fn serves_realm_status_once_pinged() {
    let s = setup().await;
    let base = format!("http://127.0.0.1:{}", s.port);
    let c = client();
    let body: Value = c.get(format!("{base}/api/realm-status")).send().await.unwrap().json().await.unwrap();
    assert!(body.is_array(), "{body:?}");
    // The background pinger hasn't necessarily finished yet; once it has,
    // every entry is one region with the shape the UI expects.
    for entry in body.as_array().unwrap() {
        assert!(entry["region"].is_string(), "{entry:?}");
        assert!(entry["host"].is_string(), "{entry:?}");
        assert!(entry["up"].is_boolean(), "{entry:?}");
        assert!(entry["checkedAt"].is_number(), "{entry:?}");
    }
}
