# Sweeps Tracker

A small desktop app for tracking sweepstakes casino purchases, coin discounts, and withdrawals.
It runs on macOS, Windows, and Linux. It's written in Rust with [egui](https://github.com/emilk/egui).

## Features
- One page per casino, with the same columns as the spreadsheet: **Date, Spent, CC Received, Withdrawal**
- The discount on every purchase is worked out for you (e.g. $85 for 100 CC = 15% off)
- Totals for each casino and for all casinos: spent, coins received, withdrawn, and **net profit**
- A chart of profit over time, plus a table comparing casinos
- **Casino directory from [SweepsGrail](https://sweepsgrail.com/)**: "+ Add casino" opens a searchable dropdown with each casino's logo, tier, signup bonus and redemption speed. Each casino page shows the SweepsGrail details (redeem speed, minimum redemptions, daily freebie, VIP, parent company) and a link to the full review. The list is downloaded in the background and refreshed once a day (`sweepsgrail.json` in the app folder, shared by all projects).
- **Paste from spreadsheet**: copy rows from Excel or Google Sheets and paste them in to import your existing data
- **Projects, like VS Code**: each project is a folder holding a SQLite database (`sweeps.db`) and a `backups/` folder. Use the project switcher at the top of the sidebar (or ⌘N / ⌘O / ⇧⌘S) to create, open, switch between, or save a copy of projects. The last project reopens on launch.
  - **Storage:** amounts are whole cents (integers), never floating point. Purchases and redemptions are separate tables; every row has `created_at` / `updated_at`, and deleting only marks a row with `deleted_at`, so history is kept for taxes. The schema version is stored in the database and upgrades run automatically.
  - Every change is its own transaction, written straight to disk. A project is locked while it's open, so a second window can't overwrite it.
  - A dated backup (`backups/sweeps-YYYY-MM-DD.db`) is made the first time a project is opened each day; the newest 30 are kept.
  - Projects saved by earlier versions (`sweeps.json`) are imported automatically the first time they're opened; the old file is kept as `sweeps.json.imported`.
  - The recent-projects list and the shared SweepsGrail cache live in the app folder: `~/Library/Application Support/SweepsTracker/` (macOS), `%APPDATA%\SweepsTracker\` (Windows), `~/.local/share/SweepsTracker/` (Linux).

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

## Test
```sh
cargo test
```
