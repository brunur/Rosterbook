# Alt Ledger

See every World of Warcraft character you own (gold, bags, bank, gear, professions, reputations, rested XP) on your PC, or on a phone or tablet beside your keyboard.

Alt Ledger has two parts:

| Part | What it does | Where it ships |
|---|---|---|
| **Add-on** (`addon/AltLedger`) | Records each character when you log out or `/reload`. Works on Retail and WoW Forever. | CurseForge, Wago, GitHub |
| **In-game panel** (`addon/AltLedger_UI`, optional) | A window in the game over the same data, for players without a second screen. Loads only when opened. | Ships in the same download as the add-on |
| **Desktop app** (`app/`, `crates/`, `ui/`) | Reads what the add-on saved, shows it in a window, and shares it with devices on your home Wi-Fi. | GitHub Releases (private while testing) |

Nothing is sent to the internet. The app runs a small web server on your own PC; phones and tablets on the same network connect to it after you approve them with a QR code.

> Alt Ledger is a fan-made tool. It is not affiliated with or endorsed by Blizzard Entertainment. World of Warcraft is a trademark of Blizzard Entertainment, Inc. The app draws its own item icons and uses no game artwork.

---

## Using it

1. Install the desktop app from the latest release on GitHub and open it.
2. Settings opens on first run with any WoW installs it found. Press **Install add-on** for each game, pick which accounts to show, and save.
3. Log in to WoW. Each character is recorded when you log out, or right away with `/reload`. Open your bank once per character to record it too.
4. To use a tablet or phone, choose **Connect a device** and scan the QR code.

Closing the window keeps Alt Ledger running in the tray so your devices stay connected. Use **Quit** in the tray menu to stop it. The tray menu also has **Start when I sign in**.

If a tablet can't connect, allow Alt Ledger through your PC's firewall on **private** networks (Windows asks the first time). The Connect screen lists other addresses to try if your PC has more than one network adapter.

**Item pictures.** By default Alt Ledger draws its own simple item icons. Settings → Games → **Show item pictures from my WoW install** switches to the game's own icons. Alt Ledger reads just the icons your characters use, straight from the game's files on your PC, and caches them. After a game patch it reads them again. Items it can't find a picture for keep the drawing.

### In game

The add-on download also contains **Alt Ledger - In-game panel**, a window over the same data for players who don't use a second screen. It's load-on-demand: it uses no memory until you open it, and you can switch it off in the AddOns list if you only want the desktop app.

- `/al`, the minimap button, or (Retail) the add-on compartment opens it. If Titan Panel, ElvUI or another add-on provides LibDataBroker, Alt Ledger shows up there too.
- Characters on the left (sort by level, name, gold, item level or last played) with total gold, and the Warband bank's gold on Retail.
- Tabs for the selected character: **Overview**, **Gear**, **Bags**, **Bank** (plus the Warband bank on Retail), **Professions**, **Reputation**, and **Find item**, which searches every character's gear, bags and bank. Hover an item for its tooltip; shift-click to link it in chat.
- **Forget this character** (on the Overview of any other character) removes a deleted or transferred character.
- The gear button opens **Settings**: background opacity, text size, window scale, window layer, lock position, keep on screen, close with Escape, only this realm's characters, and the minimap button. Settings are saved per account.
- `/al status` prints what's tracked, `/al scan` rescans, `/al minimap` hides or shows the minimap button.

The panel shows the logged-in character live; the desktop app still sees changes when you log out or `/reload`.

## How it stays private

- The server answers only on your PC and your local network. It never contacts any server of ours, because there isn't one.
- A new device must enter or scan a 6-digit code shown on the PC. Codes are single-use and expire after 5 minutes.
- Connected devices get a cookie that can't be read by scripts. The app stores only a hash of it. You can rename or remove devices, or turn network access off entirely, in Settings → Devices.
- Connected devices can view your characters and change their own view (order, filters, theme). Only the PC itself can change setup, install the add-on, or connect new devices.
- Requests that change anything need a header that other websites can't send (CSRF protection), and requests using unknown host names are refused (DNS-rebinding protection). Pages are served with a strict Content-Security-Policy.
- The desktop window loads the same page as any browser and gets no special access to your PC.
- Item pictures, when turned on, are read from the game's files without changing them, and are only served to you and your connected devices.

See [PRIVACY.md](PRIVACY.md).

---

## Development

The code is a Rust workspace:

```
addon/AltLedger/        the in-game add-on (Lua): records data, /al, launchers
addon/AltLedger_UI/     optional in-game panel (load-on-demand)
addon/test/             runs both add-ons in Lua 5.1 against a mocked WoW API
crates/core/            reads SavedVariables, settings, install detection, add-on installer
crates/casc/            read-only access to item icons in a local install (CASC storage, BLP images)
crates/server/          the local web server (axum) and its API
ui/                     the web interface (plain HTML/CSS/JS, embedded in the binary)
app/src-tauri/          the desktop app (Tauri 2): tray, window, updates
dev/make_sample.py      builds sample data for trying the UI
```

### Requirements

- Rust (stable), from <https://rustup.rs>
- Python 3 with `lupa` (`pip install lupa`), for add-on tests only
- To build the desktop app on Linux:
  - Debian/Ubuntu: `sudo apt install libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev libxdo-dev build-essential`
  - Fedora: `sudo dnf install webkit2gtk4.1-devel libappindicator-gtk3-devel librsvg2-devel libxdo-devel` and `sudo dnf group install c-development`
- Windows needs nothing extra (WebView2 ships with Windows 10/11). macOS needs Xcode's command line tools.

### Run it

Desktop app:

```sh
cargo run -p alt-ledger
```

On Linux, run `sh dev/install-desktop-entry.sh` once so the window gets Alt Ledger's icon in the taskbar and the app appears in your app menu (installed packages do this themselves).

Server only, without a window (handy while working on the UI, or on a PC you only reach from other devices):

```sh
cargo run -p alt-ledger-server -- --ui ui
```

then open <http://localhost:8787>. `--ui ui` serves the interface straight from the folder, so edits show on refresh. Add `--settings some/file.json` to keep test settings apart from your real ones.

Both share settings in your config folder (`%APPDATA%\AltLedger` on Windows, `~/Library/Application Support/AltLedger` on macOS, `~/.config/AltLedger` on Linux).

### Tests

```sh
cargo test --workspace --exclude alt-ledger
python3 addon/test/run_addon_test.py crates/core/tests/fixtures/AltLedger.forever.lua
python3 addon/test/run_addon_test.py --retail crates/core/tests/fixtures/AltLedger.retail.lua
python3 addon/test/run_panel_test.py            # in-game panel, Forever (prints each tab)
python3 addon/test/run_panel_test.py --retail   # in-game panel, Retail
```

The panel test mocks widgets strictly (an unknown widget method is an error), so it catches typos and nil errors. It can't show the layout; check that in game.

To check item pictures against a real install (it only reads):

```sh
cargo run --release -p alt-ledger-casc --example probe -- probe-output "/path/to/World of Warcraft/_retail_"
```

The two add-on runs regenerate the fixtures the Rust tests read; CI fails if they change without being committed.

---

## Releasing

### One-time setup

The repository is **private**, and so are its releases: only you can see or download them.

1. Replace `OWNER` with your GitHub name in `Cargo.toml` and `app/src-tauri/tauri.conf.json`.
2. **Add-on sites** (only when you're ready to go public). Create the project on CurseForge and Wago, then add repository variables `CURSEFORGE_PROJECT_ID` and `WAGO_PROJECT_ID`, and secrets `CF_API_KEY` (CurseForge → API tokens) and `WAGO_API_TOKEN` (Wago → account → API keys). Until then the add-on workflow only makes a GitHub release.

**Actions minutes.** Private repositories get a monthly allowance of Actions minutes on the free plan, and Windows and macOS builds cost more than Linux ones. CI on each push is Linux only and cheap; a full app release builds on all three systems, so check Settings → Billing after your first release to see how much one costs.

### Turning on automatic updates (when going public)

The app's updater downloads from GitHub without signing in, so it only works once releases are public. Until then it is off, and the tray menu says "Updates are off in this build". To turn it on:

1. Generate a signing key; installed copies only accept updates signed with it:

   ```sh
   npx @tauri-apps/cli@2 signer generate -w ~/.tauri/alt-ledger.key
   ```

2. Paste the printed **public** key into `plugins.updater.pubkey` in `app/src-tauri/tauri.conf.json`.
3. In the repo → Settings → Secrets and variables → Actions, add secrets `TAURI_SIGNING_PRIVATE_KEY` (the contents of `~/.tauri/alt-ledger.key`) and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`, and the variable `UPDATES_ENABLED` = `true`.
4. Back the key up somewhere safe. If you lose it, existing installs can never update again.

Copies installed before this point don't have updates turned on, so testers need to install the first public version by hand once.

### Desktop app

1. Bump `version` in the root `Cargo.toml`.
2. Commit, then tag and push: `git tag v0.2.0 && git push origin v0.2.0`.
3. The **Release app** workflow builds Windows, macOS (Apple silicon and Intel) and Linux installers into a draft release.
4. Check the draft, then publish it. (With updates turned on, installed copies see it within a few hours under **Install update** in the tray menu.)

### Add-on

1. Bump `## Version` in `addon/AltLedger/AltLedger.toc` and `addon/AltLedger_UI/AltLedger_UI.toc` (the desktop app bundles the same copy of the recorder and offers it as an update).
2. Commit, then tag and push: `git tag addon-v0.3.1 && git push origin addon-v0.3.1`.
3. The **Release add-on** workflow uploads it to CurseForge, Wago and GitHub.

Before the first WoW Forever release, check that CurseForge and Wago list a game version for Forever (interface `16001`). The TOC declares both Retail and Forever.

### Testing with friends

Adding someone as a collaborator on a personal repository gives them the whole source code and push access, so don't do that just to share builds. Instead:

1. Publish a release as above and mark it **pre-release**.
2. Download the installers from it yourself and send each friend the one for their system (Windows `.exe`, macOS `.dmg`, Linux `.AppImage`/`.deb`/`.rpm`) by Discord, a shared drive, or similar.
3. For each new test build, they install it over the old one. Settings and connected devices are kept.

### Code signing

The installers aren't code-signed yet. Windows SmartScreen shows "Windows protected your PC" (choose **More info → Run anyway**) and macOS refuses to open the app until it is allowed in System Settings → Privacy & Security. Signing removes those warnings and costs money: an Apple Developer membership for macOS, and a code-signing certificate (or Azure Trusted Signing) for Windows. `tauri-action` picks them up from secrets once you have them.

---

## License

Alt Ledger's desktop app, server and web interface are proprietary: © 2026 Bob, all rights reserved (see `LICENSE`). The in-game add-on in `addon/AltLedger` is MIT licensed (`addon/AltLedger/LICENSE.txt`), as add-ons on CurseForge and Wago normally are, and its Lua source ships to every player anyway.
