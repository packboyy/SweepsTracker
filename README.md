# Sweeps Tracker

A small desktop app for tracking sweepstakes casino purchases, coin discounts, and withdrawals.
It runs on macOS, Windows, and Linux. It's written in Rust with [egui](https://github.com/emilk/egui).

## Features
- One page per casino, with the same columns as the spreadsheet: **Date, Spent, CC Received, Withdrawal**, plus free SC you collect (daily logins, signup bonuses, mail-in…)
- **Withdrawals for taxes:** payout method (bank by default, gift card, crypto), fees, and pending/received status with the date the money arrived
- **Taxes page:** per-year totals by received date, payout methods, estimated deductible losses, pending payouts, a per-casino breakdown, and **CSV export** for your tax preparer (estimates, not tax advice)
- **Native macOS menu bar:** File (New / Open / Open Recent / Save As / Export CSV / Show in Finder), Edit (Undo Delete, Add Casino, Paste Rows), View (Overview ⌘1, Taxes ⌘2, Recently Deleted ⌘3, Full Screen), Window and Help. On Windows and Linux the same actions are in the sidebar project menu.
- **Dark mode:** the sun/moon button in the title bar switches between light, dark and matching the system (remembered between launches)
- **Recently deleted:** deleted casinos and entries are kept and can be restored; right after a delete, the notice bar offers **Undo**
- The discount on every purchase is worked out for you (e.g. $85 for 100 CC = 15% off)
- Totals for each casino and for all casinos: spent, coins received, withdrawn, and **net profit**
- A chart of profit over time, plus a table comparing casinos
- **Casino directory from [SweepsGrail](https://sweepsgrail.com/)**: "+ Add casino" opens a searchable dropdown with each casino's logo, tier, signup bonus and redemption speed. Each casino page shows the SweepsGrail details (redeem speed, minimum redemptions, daily freebie, VIP, parent company) and a link to the full review. The list is downloaded in the background and refreshed once a day (`sweepsgrail.json` in the app folder, shared by all projects). Columns are found by their headers, so small layout changes on the site don't break it, and a snapshot of the list is built into the app for offline use or if the site changes too much. Casino logos are saved in the app folder (`logos/`) after the first download, so they show instantly and offline.
- **Paste from spreadsheet**: copy rows from Excel or Google Sheets and paste them in to import your existing data
- **Projects, like VS Code**: each project is a folder holding a SQLite database (`sweeps.db`) and a `backups/` folder. Use the project switcher at the top of the sidebar (or ⌘N / ⌘O / ⇧⌘S) to create, open, switch between, or save a copy of projects. The last project reopens on launch.
  - **Storage:** amounts are whole cents (integers), never floating point. Purchases and redemptions are separate tables; every row has `created_at` / `updated_at`, and deleting only marks a row with `deleted_at`, so history is kept for taxes. The schema version is stored in the database and upgrades run automatically.
  - Every change is its own transaction, written straight to disk. A project is locked while it's open, so a second window can't overwrite it.
  - A dated backup (`backups/sweeps-YYYY-MM-DD.db`) is made the first time a project is opened each day; the newest 30 are kept.
  - Projects saved by earlier versions (`sweeps.json`) are imported automatically the first time they're opened; the old file is kept as `sweeps.json.imported`.
  - The recent-projects list and the shared SweepsGrail cache live in the app folder: `~/Library/Application Support/SweepsTracker/` (macOS), `%APPDATA%\SweepsTracker\` (Windows), `~/.local/share/SweepsTracker/` (Linux).

## Install on Linux with Flatpak
Download `SweepsTracker-vX.Y.Z-linux.flatpak` from the [releases](https://github.com/packboyy/SweepsTracker/releases), then:
```sh
flatpak install --user SweepsTracker-vX.Y.Z-linux.flatpak
flatpak run net.packboy.sweepstracker
```
It needs the Freedesktop runtime from Flathub (`flatpak remote-add --user --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo`); `flatpak install` offers to fetch it. Inside the Flatpak, app settings live in `~/.var/app/net.packboy.sweepstracker/`.

## Install Rust (one time)
- **macOS / Linux:** `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`
- **Windows:** download and run `rustup-init.exe` from https://rustup.rs

On Linux you may also need the GUI libraries. On Debian/Ubuntu:
`sudo apt install libxkbcommon-dev libgl1-mesa-dev libxcursor-dev libxrandr-dev libxi-dev`

## Run
```sh
cargo run --release
```
The built app is at `target/release/sweeps-tracker` (`sweeps-tracker.exe` on Windows). You can copy it anywhere and double-click it.

## Mac app
```sh
scripts/bundle-mac.sh            # builds "dist/Sweeps Tracker.app" (Apple Silicon + Intel) with the app icon
scripts/bundle-mac.sh --install  # ...and copies it to /Applications
```
The icon is `assets/icon.png`, made from the artwork in `assets/icon-source.png` with `swift scripts/make-icon.swift assets/icon-source.png assets/icon.png` (fits it into the macOS rounded-square shape).

## Updates
Click the version in the title bar (or **Check for Updates…** in the app menu) to ask GitHub for the latest [release](https://github.com/packboyy/SweepsTracker/releases). If there's a newer one, the title bar says so; click it to open the download. Only published releases count, not every commit.

To publish a release (sets the version, commits, builds the Mac app for Apple Silicon and Intel and uploads it as a zip):
```sh
scripts/release.sh 0.2.0
```
Publishing a release starts the GitHub Actions workflow in `.github/workflows/release-builds.yml`, which builds the Windows (`.zip`), Linux (`.tar.gz`) and Linux Flatpak (`.flatpak`, from `flatpak/`) versions and attaches them to the same release a few minutes later. To add them to an older release, run **Release builds** from the repository's Actions tab with that release's tag.

## Test
```sh
cargo test
```

## Code layout
- `src/main.rs` starts the app; `src/app/` has one file per screen (`sidebar`, `overview`, `casino`, `taxes`, `trash`, `picker`, `dialogs`, `project`) with shared state and helpers in `app/mod.rs`.
- `src/model.rs` data types, cents parsing/formatting; `src/store.rs` the SQLite database and schema migrations; `src/tax.rs` tax-year summary and CSV export; `src/grail.rs` SweepsGrail scraper.
- `src/theme.rs` colors, fonts and widgets; `src/titlebar.rs` the custom title bar.
- `vendor/egui_extras/` is egui_extras 0.29.1 with two fixes (the date picker's "today" uses local time; clipped table columns don't grow to fit long text) — see `[patch.crates-io]` in `Cargo.toml`.
