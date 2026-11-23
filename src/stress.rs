//! Stress and fuzz tests (test builds only). Deterministic: a fixed-seed RNG, so failures
//! reproduce. The 100k-entry timing run is `#[ignore]`d:
//!   cargo test --release stress -- --ignored --nocapture

use crate::model::*;
use crate::store::{RedemptionInput, Store};
use crate::tax::{self, TaxSummary};
use chrono::NaiveDate;
use std::time::Instant;

/// xorshift64*: small, fast, reproducible.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len() as u64) as usize]
    }
}

fn temp_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("sweeps-stress-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// "$1,234,567.80"-style formatting with optional pieces, for round-tripping.
fn format_amount(cents: i64, rng: &mut Rng) -> String {
    let dollars = cents / 100;
    let frac = cents % 100;
    let mut whole = dollars.to_string();
    if rng.below(2) == 0 {
        // thousands separators
        let digits: Vec<char> = whole.chars().collect();
        whole = digits
            .iter()
            .enumerate()
            .map(|(i, c)| {
                if i > 0 && (digits.len() - i) % 3 == 0 {
                    format!(",{c}")
                } else {
                    c.to_string()
                }
            })
            .collect();
    }
    let body = match (frac, rng.below(3)) {
        (0, 0) => whole,
        (f, _) if f % 10 == 0 && rng.below(2) == 0 => format!("{whole}.{}", f / 10),
        (f, _) => format!("{whole}.{f:02}"),
    };
    let dollar = if rng.below(2) == 0 { "$" } else { "" };
    let pad = if rng.below(4) == 0 { " " } else { "" };
    format!("{pad}{dollar}{body}{pad}")
}

#[test]
fn parse_cents_round_trips_random_amounts() {
    let mut rng = Rng(0x5eed);
    for _ in 0..50_000 {
        let cents = (rng.below(10_000_000_000)) as i64;
        let text = format_amount(cents, &mut rng);
        assert_eq!(parse_cents(&text), Some(cents), "{text:?}");
        // And formatting back for an edit box parses to the same value.
        assert_eq!(parse_cents(&edit_text(cents)).unwrap_or(0), cents);
    }
}

#[test]
fn parsers_never_panic_on_garbage() {
    let mut rng = Rng(0xbad);
    let alphabet: Vec<char> = "0123456789.,$- \t\r\n\"e+xé٣−/".chars().collect();
    for _ in 0..50_000 {
        let len = rng.below(40) as usize;
        let s: String = (0..len).map(|_| *rng.pick(&alphabet)).collect();
        if let Some(v) = parse_cents(&s) {
            // Anything accepted must be built only from digits and the allowed symbols.
            assert!(
                s.chars()
                    .all(|c| c.is_ascii_digit() || ".,$- \"".contains(c)),
                "{s:?} -> {v}"
            );
        }
        let _ = parse_date(&s);
        let _ = parse_spreadsheet(&s);
    }
    // Overflow-sized numbers are rejected, not wrapped.
    assert_eq!(parse_cents("99999999999999999999"), None);
    assert_eq!(parse_cents("92233720368547758.08"), None);
}

#[test]
fn spreadsheet_paste_handles_windows_line_endings_and_junk_rows() {
    let text = "DATE\tSPENT\tCC RECEIVED\tWITHDRAWAL\r\n\
                3/13/2026\t90\t100\t\r\n\
                totals\t1000\t1200\t\r\n\
                2/30/2026\t5\t5\t\r\n\
                3/14/2026\t\t\t500\r\n";
    let rows = parse_spreadsheet(text);
    // Header, the "totals" row and the impossible date are skipped.
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[1].withdrawal, 50000);
}

/// Inserts `n` random entries across `casinos` casinos; returns the expected totals
/// computed independently (in i128) for comparison.
fn fill(store: &mut Store, casinos: usize, n: usize, seed: u64) -> (i128, i128, i128, i128) {
    let mut rng = Rng(seed);
    let ids: Vec<i64> = (0..casinos)
        .map(|i| store.add_casino(&format!("Casino {i}")).unwrap())
        .collect();
    let (mut spent, mut cc, mut withdrawn, mut fees) = (0i128, 0i128, 0i128, 0i128);
    let base = NaiveDate::from_ymd_opt(2024, 1, 1).unwrap();
    let mut rows: Vec<Vec<ParsedRow>> = (0..casinos).map(|_| Vec::new()).collect();
    for _ in 0..n {
        let c = rng.below(casinos as u64) as usize;
        let date = base + chrono::Days::new(rng.below(3 * 365));
        if rng.below(4) == 0 {
            let amount = 1 + rng.below(200_000) as i64;
            withdrawn += amount as i128;
            rows[c].push(ParsedRow {
                date,
                spent: 0,
                cc_received: 0,
                withdrawal: amount,
            });
        } else {
            let s = 1 + rng.below(50_000) as i64;
            let coins = s + rng.below(s as u64 / 2 + 1) as i64;
            spent += s as i128;
            cc += coins as i128;
            rows[c].push(ParsedRow {
                date,
                spent: s,
                cc_received: coins,
                withdrawal: 0,
            });
        }
    }
    for (c, rows) in rows.iter().enumerate() {
        store.import_rows(ids[c], rows).unwrap();
    }
    // A few redemptions with fees and pending status through the normal path.
    for i in 0..50 {
        let fee = rng.below(500) as i64;
        let r = RedemptionInput {
            date: base + chrono::Days::new(i),
            amount: 10_000,
            method: *rng.pick(&Method::ALL),
            fee,
            received: (i % 3 != 0).then_some(base + chrono::Days::new(i + 2)),
            note: String::new(),
        };
        store.add_redemption(ids[i as usize % casinos], &r).unwrap();
        withdrawn += 10_000;
        fees += fee as i128;
    }
    (spent, cc, withdrawn, fees)
}

#[test]
fn totals_stay_exact_with_10k_entries() {
    let dir = temp_dir("exact");
    let mut store = Store::open(&dir).unwrap();
    let (spent, cc, withdrawn, fees) = fill(&mut store, 25, 10_000, 42);
    let data = store.load().unwrap();
    let s = data.stats();
    assert_eq!(s.spent as i128, spent);
    assert_eq!(s.cc_received as i128, cc);
    assert_eq!(s.withdrawn as i128, withdrawn);
    assert_eq!(s.fees as i128, fees);
    // Per-casino totals add up to the overall totals.
    let per: i64 = data.casinos.iter().map(|c| c.stats().net()).sum();
    assert_eq!(per, s.net());
    // The chart's running total ends at the overall net.
    assert_eq!(data.daily_net().values().sum::<i64>(), s.net());
    // Tax years partition the received redemptions and purchases exactly.
    let years = tax::years(&data, 2026);
    let by_year: Vec<TaxSummary> = years
        .iter()
        .map(|y| TaxSummary::for_year(&data, *y))
        .collect();
    assert_eq!(by_year.iter().map(|t| t.purchases).sum::<i64>(), s.spent);
    let pending: i64 = by_year[0].pending.iter().map(|p| p.amount).sum();
    assert_eq!(
        by_year.iter().map(|t| t.redeemed).sum::<i64>() + pending,
        s.withdrawn
    );
    // CSV has one line per entry plus the header.
    let entries: usize = data.casinos.iter().map(|c| c.entry_count()).sum();
    assert_eq!(tax::csv(&data, None).lines().count(), entries + 1);
    drop(store);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
#[ignore]
fn timing_with_100k_entries() {
    let dir = temp_dir("timing");
    let mut store = Store::open(&dir).unwrap();
    let t = Instant::now();
    fill(&mut store, 60, 100_000, 7);
    println!("insert 100k entries:     {:?}", t.elapsed());
    let t = Instant::now();
    let data = store.load().unwrap();
    println!("load:                    {:?}", t.elapsed());
    let t = Instant::now();
    let _ = (data.stats(), data.daily_net(), tax::years(&data, 2026));
    let _: Vec<_> = data.casinos.iter().map(|c| c.stats()).collect();
    println!("totals + chart + years:  {:?}", t.elapsed());
    let t = Instant::now();
    let _ = TaxSummary::for_year(&data, 2025);
    println!("tax summary (1 year):    {:?}", t.elapsed());
    let t = Instant::now();
    let csv = tax::csv(&data, None);
    println!(
        "csv export:              {:?} ({} KB)",
        t.elapsed(),
        csv.len() / 1024
    );
    let t = Instant::now();
    let deleted = store.deleted().unwrap();
    println!(
        "recently deleted query:  {:?} ({} rows)",
        t.elapsed(),
        deleted.entries.len()
    );
    let t = Instant::now();
    store.backup(&dir).unwrap();
    println!("backup (VACUUM INTO):    {:?}", t.elapsed());
    drop(store);
    std::fs::remove_dir_all(&dir).unwrap();
}

// ---- Bug hunt: each test below was written against a suspected bug ----

/// A damaged old sweeps.json must not leave behind an empty database that hides it.
#[test]
fn failed_legacy_import_can_be_retried() {
    let dir = temp_dir("legacy-fail");
    std::fs::create_dir_all(&dir).unwrap();
    let legacy = dir.join(LEGACY_PROJECT_FILE);
    std::fs::write(&legacy, "{ this is not json").unwrap();
    assert!(Store::open(&dir).is_err());
    // Fix the file and open again: the import must happen now.
    std::fs::write(
        &legacy,
        r#"{"next_id":1,"casinos":[{"id":1,"name":"Zoot","entries":[]}]}"#,
    )
    .unwrap();
    let s = Store::open(&dir).unwrap();
    assert_eq!(s.load().unwrap().casinos.len(), 1, "import was skipped");
    drop(s);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn fee_cannot_exceed_the_redemption() {
    let dir = temp_dir("fee");
    let s = Store::open(&dir).unwrap();
    let id = s.add_casino("Modo").unwrap();
    let r = RedemptionInput {
        date: NaiveDate::from_ymd_opt(2026, 3, 1).unwrap(),
        amount: 1000,
        method: Method::Bank,
        fee: 5000,
        received: None,
        note: String::new(),
    };
    assert!(
        s.add_redemption(id, &r).is_err(),
        "fee larger than the payout was accepted"
    );
    drop(s);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Notes are user text; a note like "=HYPERLINK(...)" must not become a formula when the
/// CSV is opened in Excel / Numbers / Sheets.
#[test]
fn csv_export_neutralizes_formulas() {
    let date = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
    let data = Data {
        casinos: vec![Casino {
            id: 1,
            name: "@evil".into(),
            purchases: vec![Purchase {
                id: 1,
                date,
                spent: 100,
                cc_received: 100,
                note: "=HYPERLINK(\"http://x\",\"click\")".into(),
            }],
            redemptions: vec![],
            free_coins: vec![],
        }],
    };
    let csv = tax::csv(&data, None);
    let row = csv.lines().nth(1).unwrap();
    for field in row.split(',') {
        let field = field.trim_start_matches('"');
        assert!(
            !field.starts_with(['=', '+', '@']),
            "formula-looking field exported as-is: {row}"
        );
    }
}

/// Negative numbers in a pasted sheet were counted in the preview but silently dropped on
/// import, so the preview and the result disagreed.
#[test]
fn pasted_negative_amounts_are_not_counted() {
    let rows = parse_spreadsheet("3/1/2026\t-90\t100\t\n3/2/2026\t\t\t-50");
    let spent: i64 = rows.iter().map(|r| r.spent).sum();
    let withdrawn: i64 = rows.iter().map(|r| r.withdrawal).sum();
    assert!(spent >= 0 && withdrawn >= 0, "{rows:?}");
}

/// A sweeps.db that isn't a database gives a clear error instead of a crash.
#[test]
fn opening_a_non_database_file_fails_cleanly() {
    let dir = temp_dir("notdb");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(crate::store::DB_FILE), "hello, not sqlite").unwrap();
    let err = Store::open(&dir).err().expect("should fail");
    assert!(!err.is_empty());
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Pasting a large spreadsheet: the dialog re-parses the text while it's open, so parsing
/// has to stay fast.
#[test]
fn parsing_a_big_paste_is_fast() {
    let mut text = String::from("DATE\tSPENT\tCC RECEIVED\tWITHDRAWAL\n");
    for i in 0..20_000 {
        text.push_str(&format!(
            "{}/{}/2025\t{}.50\t{}\t\n",
            i % 12 + 1,
            i % 28 + 1,
            i % 300,
            i % 400
        ));
    }
    let t = Instant::now();
    let rows = parse_spreadsheet(&text);
    let elapsed = t.elapsed();
    assert_eq!(rows.len(), 20_000);
    println!("parse 20k pasted rows: {elapsed:?}");
}

#[test]
fn detects_a_project_open_in_another_window() {
    let dir = temp_dir("in-use");
    assert!(!crate::store::in_use(&dir));
    let s = Store::open(&dir).unwrap();
    assert!(crate::store::in_use(&dir));
    drop(s);
    assert!(!crate::store::in_use(&dir));
    std::fs::remove_dir_all(&dir).unwrap();
}

// ---- Round 2: model-based operations, unusual text, extremes, upgrades, scraper ----

use crate::store::EntryRef;

/// Reference model of what the database should contain.
#[derive(Clone, Debug)]
struct RefCasino {
    id: i64,
    name: String,
    deleted: bool,
}

#[derive(Clone, Debug)]
struct RefEntry {
    entry: EntryRef,
    casino: i64,
    amount: i64,
    deleted: bool,
    /// Deleted as part of deleting its casino (restored with it).
    with_casino: bool,
}

/// Thousands of random operations against the real store, checking after every step that
/// it matches a simple reference model: active casinos and their entries, totals, and
/// what "Recently deleted" lists.
#[test]
fn random_operations_match_a_reference_model() {
    for seed in 1..=6u64 {
        let mut rng = Rng(seed * 0x9e37_79b9);
        let mut s = Store::in_memory();
        let mut casinos: Vec<RefCasino> = Vec::new();
        let mut entries: Vec<RefEntry> = Vec::new();
        let names = [
            "Modo",
            "modo",
            "Zoot",
            "ZOOT",
            "McLuck",
            "Spree",
            "Crown Coins",
            "Pulsz",
        ];
        let date = NaiveDate::from_ymd_opt(2026, 3, 1).unwrap();
        let name_taken = |casinos: &[RefCasino], name: &str, except: Option<i64>| {
            casinos
                .iter()
                .any(|c| !c.deleted && Some(c.id) != except && c.name.eq_ignore_ascii_case(name))
        };

        for step in 0..2500 {
            let active: Vec<i64> = casinos
                .iter()
                .filter(|c| !c.deleted)
                .map(|c| c.id)
                .collect();
            match rng.below(10) {
                0 => {
                    let name = rng.pick(&names).to_string();
                    let expect_err = name_taken(&casinos, &name, None);
                    let got = s.add_casino(&name);
                    assert_eq!(
                        got.is_err(),
                        expect_err,
                        "seed {seed} step {step}: add {name}"
                    );
                    if let Ok(id) = got {
                        casinos.push(RefCasino {
                            id,
                            name,
                            deleted: false,
                        });
                    }
                }
                1 if !active.is_empty() => {
                    let id = *rng.pick(&active);
                    let name = rng.pick(&names).to_string();
                    let expect_err = name_taken(&casinos, &name, Some(id));
                    let got = s.rename_casino(id, &name);
                    assert_eq!(got.is_err(), expect_err, "seed {seed} step {step}: rename");
                    if got.is_ok() {
                        casinos.iter_mut().find(|c| c.id == id).unwrap().name = name;
                    }
                }
                2..=4 if !active.is_empty() => {
                    let casino = *rng.pick(&active);
                    let amount = 1 + rng.below(100_000) as i64;
                    let entry = match rng.below(3) {
                        0 => EntryRef::Purchase(
                            s.add_purchase(casino, date, amount, amount, "").unwrap(),
                        ),
                        1 => {
                            let r = RedemptionInput {
                                date,
                                amount,
                                method: Method::Bank,
                                fee: 0,
                                received: Some(date),
                                note: String::new(),
                            };
                            EntryRef::Redemption(s.add_redemption(casino, &r).unwrap())
                        }
                        _ => EntryRef::FreeCoins(
                            s.add_free_coins(casino, date, amount, Source::Daily, "")
                                .unwrap(),
                        ),
                    };
                    entries.push(RefEntry {
                        entry,
                        casino,
                        amount,
                        deleted: false,
                        with_casino: false,
                    });
                }
                5 => {
                    let live: Vec<usize> = (0..entries.len())
                        .filter(|&i| !entries[i].deleted && active.contains(&entries[i].casino))
                        .collect();
                    if let Some(&i) = (!live.is_empty()).then(|| rng.pick(&live)) {
                        s.delete_entry(entries[i].entry).unwrap();
                        entries[i].deleted = true;
                        entries[i].with_casino = false;
                    }
                }
                6 => {
                    // Restore an entry the UI would offer: deleted on its own, casino active.
                    let gone: Vec<usize> = (0..entries.len())
                        .filter(|&i| {
                            entries[i].deleted
                                && !entries[i].with_casino
                                && active.contains(&entries[i].casino)
                        })
                        .collect();
                    if let Some(&i) = (!gone.is_empty()).then(|| rng.pick(&gone)) {
                        s.restore_entry(entries[i].entry).unwrap();
                        entries[i].deleted = false;
                    }
                }
                7 if !active.is_empty() => {
                    let id = *rng.pick(&active);
                    s.delete_casino(id).unwrap();
                    casinos.iter_mut().find(|c| c.id == id).unwrap().deleted = true;
                    for e in entries.iter_mut().filter(|e| e.casino == id && !e.deleted) {
                        e.deleted = true;
                        e.with_casino = true;
                    }
                }
                8 => {
                    let gone: Vec<i64> =
                        casinos.iter().filter(|c| c.deleted).map(|c| c.id).collect();
                    if let Some(&id) = (!gone.is_empty()).then(|| rng.pick(&gone)) {
                        let name = casinos.iter().find(|c| c.id == id).unwrap().name.clone();
                        let expect_err = name_taken(&casinos, &name, Some(id));
                        let got = s.restore_casino(id);
                        assert_eq!(
                            got.is_err(),
                            expect_err,
                            "seed {seed} step {step}: restore casino"
                        );
                        if got.is_ok() {
                            casinos.iter_mut().find(|c| c.id == id).unwrap().deleted = false;
                            for e in entries
                                .iter_mut()
                                .filter(|e| e.casino == id && e.with_casino)
                            {
                                e.deleted = false;
                                e.with_casino = false;
                            }
                        }
                    }
                }
                _ => {}
            }
            check_against_model(&s, &casinos, &entries, seed, step);
        }
    }
}

fn check_against_model(
    s: &Store,
    casinos: &[RefCasino],
    entries: &[RefEntry],
    seed: u64,
    step: usize,
) {
    let ctx = format!("seed {seed} step {step}");
    let data = s.load().unwrap();
    let mut want: Vec<(i64, String)> = casinos
        .iter()
        .filter(|c| !c.deleted)
        .map(|c| (c.id, c.name.clone()))
        .collect();
    let mut got: Vec<(i64, String)> = data
        .casinos
        .iter()
        .map(|c| (c.id, c.name.clone()))
        .collect();
    want.sort();
    got.sort();
    assert_eq!(got, want, "{ctx}: active casinos");
    for c in &data.casinos {
        let live = |kind: fn(&EntryRef) -> bool| -> Vec<(i64, i64)> {
            let mut v: Vec<(i64, i64)> = entries
                .iter()
                .filter(|e| e.casino == c.id && !e.deleted && kind(&e.entry))
                .map(|e| (e.entry_id(), e.amount))
                .collect();
            v.sort();
            v
        };
        let sorted = |mut v: Vec<(i64, i64)>| {
            v.sort();
            v
        };
        assert_eq!(
            sorted(c.purchases.iter().map(|p| (p.id, p.spent)).collect()),
            live(|e| matches!(e, EntryRef::Purchase(_))),
            "{ctx}: purchases of {}",
            c.name
        );
        assert_eq!(
            sorted(c.redemptions.iter().map(|r| (r.id, r.amount)).collect()),
            live(|e| matches!(e, EntryRef::Redemption(_))),
            "{ctx}: redemptions of {}",
            c.name
        );
        assert_eq!(
            sorted(c.free_coins.iter().map(|f| (f.id, f.sc)).collect()),
            live(|e| matches!(e, EntryRef::FreeCoins(_))),
            "{ctx}: free coins of {}",
            c.name
        );
    }
    // Recently deleted: deleted casinos (with how many entries went with them), and
    // entries deleted on their own from casinos that still exist.
    let deleted = s.deleted().unwrap();
    let mut want_casinos: Vec<(i64, i64)> = casinos
        .iter()
        .filter(|c| c.deleted)
        .map(|c| {
            (
                c.id,
                entries
                    .iter()
                    .filter(|e| e.casino == c.id && e.with_casino)
                    .count() as i64,
            )
        })
        .collect();
    let mut got_casinos: Vec<(i64, i64)> =
        deleted.casinos.iter().map(|c| (c.id, c.entries)).collect();
    want_casinos.sort();
    got_casinos.sort();
    assert_eq!(got_casinos, want_casinos, "{ctx}: deleted casinos");
    let active = |id: i64| casinos.iter().any(|c| c.id == id && !c.deleted);
    let mut want_entries: Vec<String> = entries
        .iter()
        .filter(|e| e.deleted && active(e.casino))
        .map(|e| format!("{:?}", e.entry))
        .collect();
    let mut got_entries: Vec<String> = deleted
        .entries
        .iter()
        .map(|e| format!("{:?}", e.entry))
        .collect();
    want_entries.sort();
    got_entries.sort();
    assert_eq!(got_entries, want_entries, "{ctx}: deleted entries");
}

impl RefEntry {
    fn entry_id(&self) -> i64 {
        match self.entry {
            EntryRef::Purchase(id) | EntryRef::Redemption(id) | EntryRef::FreeCoins(id) => id,
        }
    }
}

#[test]
fn unusual_names_notes_and_folder_paths_round_trip() {
    let names = [
        "Modo 🎰",
        "Café Casino",
        "'; DROP TABLE casinos; --",
        "Robert\"); DROP TABLE purchases;--",
        "كازينو",
        "Zero\u{200b}Width",
        &"Long".repeat(200),
        "   spaced   ",
        "tab\there",
    ];
    let dir = temp_dir("Andy's Sweeps 🎰 [2026] (copy)");
    let s = Store::open(&dir).unwrap();
    let date = NaiveDate::from_ymd_opt(2026, 2, 28).unwrap();
    for (i, name) in names.iter().enumerate() {
        let id = s.add_casino(name).unwrap();
        let note = format!("line one\nline \"two\", {name}");
        s.add_purchase(id, date, 100 + i as i64, 120, &note)
            .unwrap();
    }
    let data = s.load().unwrap();
    assert_eq!(data.casinos.len(), names.len());
    for name in names {
        let c = data.casinos.iter().find(|c| c.name == name).expect(name);
        assert!(c.purchases[0].note.ends_with(name));
    }
    // Tables are intact after the injection-looking names.
    assert_eq!(s.load().unwrap().casinos.len(), names.len());
    // CSV quotes every field that needs it: each entry is one record even with newlines.
    let csv = tax::csv(&data, None);
    assert_eq!(csv.matches("Purchase,").count(), names.len());
    // Backups and Save As copies work in folders with quotes, brackets and emoji.
    s.backup(&dir).unwrap();
    let copy_dir = temp_dir("copy to 'here' 🎲");
    std::fs::create_dir_all(&copy_dir).unwrap();
    s.copy_to(&crate::store::db_path(&copy_dir)).unwrap();
    drop(s);
    let copy = Store::open(&copy_dir).unwrap();
    assert_eq!(copy.load().unwrap().casinos.len(), names.len());
    drop(copy);
    std::fs::remove_dir_all(&dir).unwrap();
    std::fs::remove_dir_all(&copy_dir).unwrap();
}

#[test]
fn extreme_dates_work_everywhere() {
    let s = Store::in_memory();
    let id = s.add_casino("Time").unwrap();
    let dates = [
        NaiveDate::from_ymd_opt(1, 1, 1).unwrap(),
        NaiveDate::from_ymd_opt(2024, 2, 29).unwrap(),
        NaiveDate::from_ymd_opt(2025, 12, 31).unwrap(),
        NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
        NaiveDate::from_ymd_opt(9999, 12, 31).unwrap(),
    ];
    for d in dates {
        s.add_purchase(id, d, 100, 100, "").unwrap();
        let r = RedemptionInput {
            date: d,
            amount: 500,
            method: Method::Crypto,
            fee: 0,
            received: Some(d),
            note: String::new(),
        };
        s.add_redemption(id, &r).unwrap();
    }
    let data = s.load().unwrap();
    let got: Vec<NaiveDate> = data.casinos[0].purchases.iter().map(|p| p.date).collect();
    assert_eq!(got, dates);
    let years = tax::years(&data, 2026);
    assert_eq!(years, vec![9999, 2026, 2025, 2024, 1]);
    // Year boundaries: Dec 31 and Jan 1 land in their own years.
    assert_eq!(TaxSummary::for_year(&data, 2025).redeemed, 500);
    assert_eq!(TaxSummary::for_year(&data, 2026).redeemed, 500);
    assert_eq!(data.daily_net().len(), dates.len());
    assert!(tax::csv(&data, Some(1)).contains("0001-01-01"));
}

/// Amounts big enough to overflow totals must be refused, not wrap around or crash.
#[test]
fn absurd_amounts_are_refused() {
    // Typed / pasted.
    assert_eq!(parse_cents("$1,000,000,000.01"), None);
    assert!(parse_cents("$1,000,000,000").is_some());
    // Old JSON projects with impossible values fail to import cleanly.
    let dir = temp_dir("huge-legacy");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join(LEGACY_PROJECT_FILE),
        r#"{"next_id":2,"casinos":[{"id":1,"name":"Big","entries":[
            {"id":1,"date":"2026-01-01","spent":1e300,"cc_received":0,"withdrawal":0},
            {"id":2,"date":"2026-01-02","spent":1e300,"cc_received":0,"withdrawal":0}]}]}"#,
    )
    .unwrap();
    assert!(Store::open(&dir).is_err());
    std::fs::remove_dir_all(&dir).unwrap();
    // Directly through the store too.
    let s = Store::in_memory();
    let id = s.add_casino("Big").unwrap();
    assert!(s
        .add_purchase(
            id,
            NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
            i64::MAX,
            0,
            ""
        )
        .is_err());
}

/// A version-1 database full of data upgrades with every value, note and deletion intact.
#[test]
fn upgrading_a_full_version_1_database_keeps_everything() {
    let dir = temp_dir("v1-upgrade");
    std::fs::create_dir_all(&dir).unwrap();
    {
        let conn = rusqlite::Connection::open(crate::store::db_path(&dir)).unwrap();
        conn.execute_batch(crate::store::MIGRATIONS[0]).unwrap();
        conn.pragma_update(None, "user_version", 1).unwrap();
        conn.execute_batch(
            "INSERT INTO casinos (id, name) VALUES (1, 'Modo'), (2, 'Zoot 🎰');
             INSERT INTO casinos (id, name, deleted_at) VALUES (3, 'Gone', '2026-03-05T10:00:00.000Z');
             INSERT INTO purchases (casino_id, date, spent_cents, cc_received_cents, note)
                 VALUES (1, '2026-03-01', 8500, 10000, 'first, \"quoted\"'),
                        (2, '2026-03-02', 1999, 2000, '');
             INSERT INTO purchases (casino_id, date, spent_cents, cc_received_cents, deleted_at)
                 VALUES (1, '2026-03-03', 500, 500, '2026-03-04T09:00:00.000Z'),
                        (3, '2026-02-01', 100, 100, '2026-03-05T10:00:00.000Z');
             INSERT INTO redemptions (casino_id, date, amount_cents, note)
                 VALUES (1, '2026-03-10', 57700, 'paid'), (2, '2026-03-11', 12345, '');",
        )
        .unwrap();
    }
    let mut s = Store::open(&dir).unwrap();
    let data = s.load().unwrap();
    let modo = data.casinos.iter().find(|c| c.name == "Modo").unwrap();
    assert_eq!(modo.purchases.len(), 1);
    assert_eq!(modo.purchases[0].note, "first, \"quoted\"");
    assert_eq!(modo.redemptions[0].amount, 57700);
    assert_eq!(
        modo.redemptions[0].received,
        modo.redemptions[0].date.into()
    );
    assert_eq!(modo.redemptions[0].method, Method::Bank);
    let zoot = data.casinos.iter().find(|c| c.name == "Zoot 🎰").unwrap();
    assert_eq!(
        (zoot.purchases[0].spent, zoot.redemptions[0].amount),
        (1999, 12345)
    );
    // Deletions survive: the purchase deleted on its own is listed, the casino too.
    let deleted = s.deleted().unwrap();
    assert_eq!(deleted.entries.len(), 1);
    assert_eq!(deleted.casinos.len(), 1);
    assert_eq!(deleted.casinos[0].entries, 1);
    // And restoring the old casino brings its entry back.
    s.restore_casino(3).unwrap();
    let gone = s
        .load()
        .unwrap()
        .casinos
        .into_iter()
        .find(|c| c.id == 3)
        .unwrap();
    assert_eq!(gone.purchases.len(), 1);
    drop(s);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// The real SweepsGrail page, cut off at hundreds of random points, never crashes the
/// parser (set GRAIL_HTML to a saved copy; skipped otherwise).
#[test]
fn scraper_survives_truncated_and_mangled_pages() {
    let Ok(path) = std::env::var("GRAIL_HTML") else {
        return;
    };
    let html = std::fs::read_to_string(path).unwrap();
    let mut rng = Rng(0x617a);
    for _ in 0..300 {
        let mut cut = rng.below(html.len() as u64) as usize;
        while !html.is_char_boundary(cut) {
            cut -= 1;
        }
        let _ = crate::grail::parse(&html[..cut]);
    }
    // Remove random chunks from the middle too.
    for _ in 0..100 {
        let mut a = rng.below(html.len() as u64) as usize;
        let mut b = (a + rng.below(5000) as usize).min(html.len());
        while !html.is_char_boundary(a) {
            a -= 1;
        }
        while !html.is_char_boundary(b) {
            b -= 1;
        }
        let mangled = format!("{}{}", &html[..a], &html[b..]);
        let _ = crate::grail::parse(&mangled);
    }
    assert!(crate::grail::parse(&html).len() > 50);
}

// ---- Round 3 ----

/// Rows written the way Excel, Google Sheets and CSV exports write them (thousands
/// separators, "$", quoted CSV cells, two- and four-digit years) parse back exactly.
#[test]
fn pasted_rows_round_trip_in_every_spreadsheet_format() {
    let mut rng = Rng(0x7ab1e);
    for _ in 0..2_000 {
        let csv = rng.below(2) == 0;
        let mut text = String::new();
        let mut expected = Vec::new();
        for _ in 0..rng.below(20) + 1 {
            let date = NaiveDate::from_ymd_opt(
                2000 + rng.below(69) as i32,
                1 + rng.below(12) as u32,
                1 + rng.below(28) as u32,
            )
            .unwrap();
            let date_text = match rng.below(3) {
                0 => date.format("%-m/%-d/%y").to_string(),
                1 => date.format("%-m/%-d/%Y").to_string(),
                _ => date.format("%Y-%m-%d").to_string(),
            };
            let amounts: Vec<i64> = (0..3)
                .map(|_| match rng.below(3) {
                    0 => 0,
                    _ => rng.below(100_000_000) as i64 + 1,
                })
                .collect();
            if amounts.iter().all(|a| *a == 0) {
                continue;
            }
            let cells: Vec<String> = amounts
                .iter()
                .map(|&a| {
                    if a == 0 {
                        return String::new();
                    }
                    let s = format_amount(a, &mut rng);
                    // CSV writers quote any cell holding a comma (and some quote everything).
                    if csv && (s.contains(',') || rng.below(4) == 0) {
                        format!("\"{s}\"")
                    } else {
                        s
                    }
                })
                .collect();
            let sep = if csv { "," } else { "\t" };
            text.push_str(&format!("{date_text}{sep}{}\n", cells.join(sep)));
            expected.push(ParsedRow {
                date,
                spent: amounts[0],
                cc_received: amounts[1],
                withdrawal: amounts[2],
            });
        }
        assert_eq!(parse_spreadsheet(&text), expected, "{text}");
    }
}

#[test]
fn two_digit_years_are_this_century() {
    let d = |y, m, day| NaiveDate::from_ymd_opt(y, m, day);
    assert_eq!(parse_date("3/18/26"), d(2026, 3, 18));
    assert_eq!(parse_date("12/31/99"), d(1999, 12, 31));
    assert_eq!(parse_date("3/18/2026"), d(2026, 3, 18));
    assert_eq!(parse_date("2026-03-18"), d(2026, 3, 18));
    assert_eq!(parse_date("1/1/0001"), d(1, 1, 1));
    // An imported "26" row is a 2026 tax year, not year 26.
    let rows = parse_spreadsheet("3/18/26\t10\t10\t");
    let data = Data {
        casinos: vec![Casino {
            id: 1,
            name: "A".into(),
            purchases: vec![Purchase {
                id: 1,
                date: rows[0].date,
                spent: rows[0].spent,
                cc_received: rows[0].cc_received,
                note: String::new(),
            }],
            redemptions: vec![],
            free_coins: vec![],
        }],
    };
    assert_eq!(tax::years(&data, 2026), vec![2026]);
}

#[test]
fn csv_quotes_hold_commas_and_escaped_quotes() {
    let rows = parse_spreadsheet("3/13/2026,\"$1,234.50\",\"1,500\",\"2,000\"\n");
    assert_eq!(rows.len(), 1);
    assert_eq!(
        (rows[0].spent, rows[0].cc_received, rows[0].withdrawal),
        (123450, 150000, 200000)
    );
    // A doubled quote inside a quoted cell is a literal quote, not the end of the cell.
    let rows = parse_spreadsheet("\"3/13/2026\",\"90\",\"100\",\"\"\"\"");
    assert_eq!((rows[0].spent, rows[0].cc_received, rows[0].withdrawal), (9000, 10000, 0));
}

/// An old project whose casino names differ only by case used to fail to open at all.
#[test]
fn legacy_names_differing_by_case_are_kept_apart() {
    let dir = temp_dir("legacy-case");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join(LEGACY_PROJECT_FILE),
        r#"{"casinos":[
            {"name":"Modo","entries":[{"date":"2026-01-01","spent":10,"cc_received":10,"withdrawal":0}]},
            {"name":"modo","entries":[{"date":"2026-01-02","spent":20,"cc_received":20,"withdrawal":0}]},
            {"name":"MODO","entries":[]},
            {"name":"modo (2)","entries":[]}
        ]}"#,
    )
    .unwrap();
    let s = Store::open(&dir).expect("old project opens");
    let data = s.load().unwrap();
    let mut names: Vec<&str> = data.casinos.iter().map(|c| c.name.as_str()).collect();
    names.sort();
    assert_eq!(names, ["MODO (3)", "Modo", "modo (2)", "modo (2) (2)"]);
    assert_eq!(data.stats().spent, 3000, "no entries lost");
    drop(s);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_tiny_markup_is_not_minus_zero_percent() {
    assert_eq!(percent(-0.0001), "0%");
    assert_eq!(percent(-0.0004), "0%");
    assert_eq!(percent(-0.001), "-0.1%");
    assert_eq!(percent(0.0), "0%");
    assert_eq!(percent(1.0), "100%");
}

// ---- Round 4: quitting, crashing and reopening ----
//
// These start a second copy of the app as a real process (this test binary, run again in
// `child_process` mode), kill it at random moments like a crash or Force Quit, then reopen
// the project here.

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

/// A child process and the `@@`-marked lines it prints as it works.
struct Child {
    process: std::process::Child,
    lines: mpsc::Receiver<String>,
}

impl Child {
    fn spawn(mode: &str, dir: &Path) -> Child {
        Child::spawn_with(mode, dir, &[])
    }

    /// With extra environment variables (e.g. HOME, so the app folder is a temp folder).
    fn spawn_with(mode: &str, dir: &Path, env: &[(&str, &str)]) -> Child {
        let mut process = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["stress::child_process", "--exact", "--ignored", "--nocapture"])
            .env("SWEEPS_CHILD", mode)
            .env("SWEEPS_DIR", dir)
            .envs(env.iter().copied())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let stdout = process.stdout.take().unwrap();
        let (tx, lines) = mpsc::channel();
        std::thread::spawn(move || {
            for line in std::io::BufReader::new(stdout).lines().map_while(Result::ok) {
                // libtest can print its own text on the same line, so find the marker.
                if let Some(at) = line.find("@@") {
                    let _ = tx.send(line[at + 2..].to_string());
                }
            }
        });
        Child { process, lines }
    }

    /// Waits for the child to print `what`; panics after 30 s.
    fn wait_for(&self, what: &str) {
        loop {
            let line = self.lines.recv_timeout(Duration::from_secs(30)).expect(what);
            if line == what {
                return;
            }
        }
    }

    /// Kills it the hard way (SIGKILL: no cleanup runs, like a crash) and returns what it
    /// reported before dying.
    fn kill(mut self) -> Vec<String> {
        let _ = self.process.kill();
        let _ = self.process.wait();
        // The reader thread ends when the pipe closes.
        let mut seen = Vec::new();
        while let Ok(line) = self.lines.recv_timeout(Duration::from_secs(5)) {
            seen.push(line);
        }
        seen
    }
}

fn say(line: &str) {
    let mut out = std::io::stdout().lock();
    writeln!(out, "@@{line}").unwrap();
    out.flush().unwrap();
}

/// Not a real test: the body of the child processes above (does nothing when run normally).
#[test]
#[ignore]
fn child_process() {
    let Ok(mode) = std::env::var("SWEEPS_CHILD") else {
        return;
    };
    let dir = PathBuf::from(std::env::var("SWEEPS_DIR").unwrap());
    let date = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
    match mode.as_str() {
        // Adds purchases one at a time (note "<pid>-<n>") and in 100-row pastes, reporting
        // each after it's saved.
        "write" => {
            let mut s = Store::open(&dir).unwrap();
            say("open");
            let id = s.load().unwrap().casinos[0].id;
            let pid = std::process::id();
            for n in 1.. {
                if n % 10 == 0 {
                    let row = || ParsedRow {
                        date,
                        spent: 1,
                        cc_received: 1,
                        withdrawal: 0,
                    };
                    s.import_rows(id, &(0..100).map(|_| row()).collect::<Vec<_>>())
                        .unwrap();
                    say("batch");
                } else {
                    let note = format!("{pid}-{n}");
                    s.add_purchase(id, date, n, n, &note).unwrap();
                    say(&format!("one {note}"));
                }
            }
        }
        // What the app does when it opens a project: open (migrate / import) and back up.
        "open" => {
            let s = Store::open(&dir).unwrap();
            s.backup(&dir).unwrap();
            say("done");
        }
        // Saves the app settings (in $HOME's app folder) over and over; each copy writes
        // its own recent list of SWEEPS_LEN paths.
        "config" => {
            let len: usize = std::env::var("SWEEPS_LEN").unwrap().parse().unwrap();
            let pid = std::process::id();
            let config = Config {
                recent: (0..len).map(|i| PathBuf::from(format!("/{pid}/{i}"))).collect(),
            };
            for _ in 0..2_000 {
                config.save().unwrap();
            }
            say("done");
        }
        // Saves a SweepsGrail cache of SWEEPS_LEN casinos to SWEEPS_DIR/sweepsgrail.json.
        "grail" => {
            let len: usize = std::env::var("SWEEPS_LEN").unwrap().parse().unwrap();
            let mut db = crate::grail::GrailDb::load(Path::new("/nonexistent"));
            db.fetched_at = std::process::id() as i64;
            db.casinos.truncate(len);
            for _ in 0..500 {
                db.save(&dir.join("sweepsgrail.json")).unwrap();
            }
            say("done");
        }
        // Two windows, both started before either opened a project, each open one.
        "two-windows" => {
            let mut a = Config::load();
            let mut b = Config::load();
            a.update(|c| c.push_recent(Path::new("/projects/X"))).unwrap();
            b.update(|c| c.push_recent(Path::new("/projects/Y"))).unwrap();
            let saved = Config::load().recent;
            say(&format!("recent {}", saved.len()));
        }
        // Keeps the project open until killed.
        "hold" => {
            let _s = Store::open(&dir).unwrap();
            say("open");
            loop {
                std::thread::sleep(Duration::from_secs(1));
            }
        }
        _ => panic!("unknown mode {mode}"),
    }
}

fn integrity(db: &Path) -> String {
    rusqlite::Connection::open_with_flags(db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .and_then(|c| c.query_row("PRAGMA integrity_check", [], |r| r.get::<_, String>(0)))
        .unwrap_or_else(|e| e.to_string())
}

/// Killed mid-save, over and over: the project always reopens, undamaged, with every entry
/// the app had finished saving, and pastes are all-or-nothing.
#[test]
fn crashing_while_saving_keeps_everything_saved() {
    let dir = temp_dir("crash-write");
    Store::open(&dir).unwrap().add_casino("A").unwrap();
    let mut rng = Rng(0xc4a5);
    let mut saved: Vec<String> = Vec::new();
    let mut batches = 0;
    for round in 0..25 {
        let child = Child::spawn("write", &dir);
        child.wait_for("open");
        std::thread::sleep(Duration::from_millis(rng.below(80)));
        for line in child.kill() {
            match line.strip_prefix("one ") {
                Some(note) => saved.push(note.to_string()),
                None if line == "batch" => batches += 1,
                None => {}
            }
        }

        let s = Store::open(&dir).unwrap_or_else(|e| panic!("round {round}: {e}"));
        let c = &s.load().unwrap().casinos[0];
        let notes: std::collections::HashSet<&str> =
            c.purchases.iter().map(|p| p.note.as_str()).collect();
        for note in &saved {
            assert!(notes.contains(note.as_str()), "round {round}: lost {note}");
        }
        let pasted = c.purchases.iter().filter(|p| p.note.is_empty()).count();
        assert_eq!(pasted % 100, 0, "round {round}: half a paste was saved");
        // At most one more paste than reported (saved, then killed before it said so).
        assert!((batches..=batches + 1).contains(&(pasted / 100)));
        batches = pasted / 100;
        drop(s);
        assert_eq!(integrity(&crate::store::db_path(&dir)), "ok", "round {round}");
    }
    assert!(saved.len() > 50, "the child barely ran: {}", saved.len());
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A second running copy of the app is really refused (the lock tests above are inside
/// one process), and a crashed copy doesn't leave the project locked.
#[test]
fn another_running_copy_is_refused_and_a_crash_releases_the_lock() {
    let dir = temp_dir("crash-lock");
    Store::open(&dir).unwrap();
    let child = Child::spawn("hold", &dir);
    child.wait_for("open");
    assert!(crate::store::in_use(&dir));
    assert_eq!(
        Store::open(&dir).err().unwrap(),
        "This project is already open in another Sweeps Tracker window."
    );
    child.kill();
    assert!(!crate::store::in_use(&dir), "still locked after the other copy crashed");
    Store::open(&dir).unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Killed during the first open of an old sweeps.json project (while it's imported): the
/// next open must still import it, once.
#[test]
fn crashing_during_an_old_project_import_still_imports_it() {
    let entries: Vec<String> = (0..20_000)
        .map(|i| format!(r#"{{"date":"2026-01-01","spent":1,"cc_received":1,"withdrawal":0,"note":"{i}"}}"#))
        .collect();
    let json = format!(r#"{{"casinos":[{{"name":"Old","entries":[{}]}}]}}"#, entries.join(","));
    let dir = temp_dir("crash-import");
    let fresh = |dir: &Path| {
        let _ = std::fs::remove_dir_all(dir);
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join(LEGACY_PROJECT_FILE), &json).unwrap();
    };
    // How long a whole open takes, so the kills land all through it.
    fresh(&dir);
    let start = Instant::now();
    Child::spawn("open", &dir).wait_for("done");
    let full = start.elapsed().as_millis() as u64;

    let mut rng = Rng(0x1e9a);
    for round in 0..30 {
        fresh(&dir);
        let child = Child::spawn("open", &dir);
        std::thread::sleep(Duration::from_millis(rng.below(full + 1)));
        child.kill();
        let s = Store::open(&dir).unwrap_or_else(|e| panic!("round {round}: {e}"));
        let data = s.load().unwrap();
        let imported: usize = data.casinos.iter().map(|c| c.purchases.len()).sum();
        assert_eq!(
            (data.casinos.len(), imported),
            (1, 20_000),
            "round {round}: old project not imported (sweeps.json still there: {})",
            dir.join(LEGACY_PROJECT_FILE).exists()
        );
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Killed while the daily backup is being written: there must never be a damaged backup
/// left behind (the next open the same day would see it and skip the backup).
#[test]
fn crashing_during_the_daily_backup_leaves_no_broken_backup() {
    let dir = temp_dir("crash-backup");
    {
        let mut s = Store::open(&dir).unwrap();
        fill(&mut s, 20, 300_000, 0xbac);
    }
    let backups = dir.join("backups");
    let _ = std::fs::remove_dir_all(&backups);
    let start = Instant::now();
    Child::spawn("open", &dir).wait_for("done");
    let full = start.elapsed().as_millis() as u64;

    let mut rng = Rng(0xbac0);
    for round in 0..30 {
        let _ = std::fs::remove_dir_all(&backups);
        let child = Child::spawn("open", &dir);
        std::thread::sleep(Duration::from_millis(rng.below(full + 1)));
        child.kill();
        for file in std::fs::read_dir(&backups).into_iter().flatten().flatten() {
            let path = file.path();
            if path.extension().is_some_and(|e| e == "db") {
                assert_eq!(integrity(&path), "ok", "round {round}: {}", path.display());
            }
        }
        assert_eq!(integrity(&crate::store::db_path(&dir)), "ok");
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Opening and closing a project many times doesn't leak open files or lose anything.
#[test]
fn opening_and_closing_hundreds_of_times() {
    let dir = temp_dir("reopen");
    let open_files = || std::fs::read_dir("/dev/fd").map(|d| d.count()).unwrap_or(0);
    let id = Store::open(&dir).unwrap().add_casino("A").unwrap();
    let before = open_files();
    let date = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
    for n in 1..=500 {
        let s = Store::open(&dir).unwrap();
        s.add_purchase(id, date, n, n, "").unwrap();
        if n % 50 == 0 {
            s.backup(&dir).unwrap();
        }
    }
    // Other tests run alongside and open files too; a real leak here would be one or more
    // per open, so hundreds.
    assert!(open_files() <= before + 50, "{before} -> {} open files", open_files());
    let s = Store::open(&dir).unwrap();
    assert_eq!(s.load().unwrap().casinos[0].purchases.len(), 500);
    drop(s);
    std::fs::remove_dir_all(&dir).unwrap();
}

// ---- Round 5: several app windows sharing the app folder ----

/// The app folder under a fake home directory (what `app_dir` gives with HOME=`home`).
fn app_dir_in(home: &Path) -> PathBuf {
    if cfg!(target_os = "macos") {
        home.join("Library/Application Support/SweepsTracker")
    } else {
        home.join(".local/share/SweepsTracker")
    }
}

/// Runs `copies` writer processes (`mode`, each with a different SWEEPS_LEN) while reading
/// `file` as fast as possible; every read must parse with `check`. Returns how many reads
/// were made.
fn race_writers(mode: &str, dir: &Path, home: &Path, file: &Path, check: impl Fn(&str) -> Result<(), String>) -> usize {
    let lens = ["1", "40", "3", "90"];
    let children: Vec<Child> = lens
        .iter()
        .map(|len| {
            Child::spawn_with(mode, dir, &[("HOME", home.to_str().unwrap()), ("SWEEPS_LEN", len)])
        })
        .collect();
    let mut reads = 0;
    let mut finished = 0;
    while finished < children.len() {
        if let Ok(text) = std::fs::read_to_string(file) {
            reads += 1;
            if let Err(e) = check(&text) {
                for c in children {
                    c.kill();
                }
                panic!("read {reads}: {e}\n{}", &text[..text.len().min(300)]);
            }
        }
        finished = children
            .iter()
            .filter(|c| c.lines.try_recv().is_ok_and(|l| l == "done"))
            .count()
            .max(finished);
        if finished < children.len() && children.iter().all(|c| matches!(c.lines.try_recv(), Err(mpsc::TryRecvError::Disconnected))) {
            break;
        }
    }
    for c in children {
        c.kill();
    }
    reads
}

/// Two windows saving the settings at the same moment must never leave a damaged
/// config.json (which loads as empty: the recent projects list would be gone).
#[test]
fn windows_saving_settings_at_once_never_damage_them() {
    let home = temp_dir("shared-config");
    let file = app_dir_in(&home).join("config.json");
    let reads = race_writers("config", &home, &home, &file, |text| {
        let config: Config = serde_json::from_str(text).map_err(|e| e.to_string())?;
        // All paths must come from the same writer.
        let owner = |p: &PathBuf| p.iter().nth(1).map(|s| s.to_owned());
        let first = config.recent.first().map(owner);
        if config.recent.iter().any(|p| Some(owner(p)) != first) {
            return Err("recent list mixes two windows' saves".into());
        }
        Ok(())
    });
    assert!(reads > 100, "only {reads} reads");
    std::fs::remove_dir_all(&home).unwrap();
}

/// Same for the shared SweepsGrail cache (a damaged one falls back to the built-in list).
#[test]
fn windows_saving_the_casino_list_at_once_never_damage_it() {
    let home = temp_dir("shared-grail");
    std::fs::create_dir_all(&home).unwrap();
    let file = home.join("sweepsgrail.json");
    let reads = race_writers("grail", &home, &home, &file, |text| {
        let db: crate::grail::GrailDb = serde_json::from_str(text).map_err(|e| e.to_string())?;
        if db.casinos.is_empty() {
            return Err("empty list".into());
        }
        Ok(())
    });
    assert!(reads > 100, "only {reads} reads");
    std::fs::remove_dir_all(&home).unwrap();
}

/// A window that has been open a while must not drop projects another window added to
/// Recent when it saves its own change.
#[test]
fn two_windows_both_keep_their_recent_projects() {
    let home = temp_dir("two-windows");
    let child = Child::spawn_with("two-windows", &home, &[("HOME", home.to_str().unwrap())]);
    let line = child.lines.recv_timeout(Duration::from_secs(30)).unwrap();
    child.kill();
    assert_eq!(line, "recent 2", "a window's project was dropped from Recent");
    std::fs::remove_dir_all(&home).unwrap();
}

// ---- Round 6: damaged project files ----

/// A project file damaged in every way we can think of (flipped bits, cut short, a page
/// zeroed or overwritten with another page, garbage header) must give an error, never
/// crash: the app reopens the last project at launch, so a crash here would repeat on
/// every launch.
#[test]
fn damaged_project_files_never_crash() {
    let dir = temp_dir("damaged-src");
    {
        let mut s = Store::open(&dir).unwrap();
        fill(&mut s, 8, 3_000, 0xda3);
        let c = s.add_casino("Deleted").unwrap();
        s.add_purchase(c, NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(), 5, 5, "x")
            .unwrap();
        s.delete_casino(c).unwrap();
    }
    let good = std::fs::read(crate::store::db_path(&dir)).unwrap();
    let page = 4096;
    let target = temp_dir("damaged");
    let mut rng = Rng(0xda3a);
    let (mut opened, mut refused) = (0, 0);
    for round in 0..400 {
        let mut bytes = good.clone();
        let how = rng.below(6);
        match how {
            0 => {
                for _ in 0..1 + rng.below(20) {
                    let i = rng.below(bytes.len() as u64) as usize;
                    bytes[i] ^= 1 << rng.below(8);
                }
            }
            1 => bytes.truncate(rng.below(bytes.len() as u64) as usize),
            2 => {
                let p = rng.below((bytes.len() / page) as u64) as usize * page;
                bytes[p..p + page].fill(0);
            }
            3 => {
                let pages = bytes.len() / page;
                let (from, to) = (rng.below(pages as u64) as usize, rng.below(pages as u64) as usize);
                let copy = bytes[from * page..(from + 1) * page].to_vec();
                bytes[to * page..(to + 1) * page].copy_from_slice(&copy);
            }
            4 => {
                // The 100-byte header: page size, counts, schema cookie, user_version...
                for _ in 0..1 + rng.below(4) {
                    let i = rng.below(100) as usize;
                    bytes[i] = rng.below(256) as u8;
                }
            }
            _ => {
                let i = rng.below(bytes.len() as u64) as usize;
                let end = (i + 1 + rng.below(512) as usize).min(bytes.len());
                for b in &mut bytes[i..end] {
                    *b = rng.below(256) as u8;
                }
            }
        }
        let _ = std::fs::remove_dir_all(&target);
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(crate::store::db_path(&target), &bytes).unwrap();
        // Everything the app does when it opens a project.
        let outcome = std::panic::catch_unwind(|| match Store::open(&target) {
            Ok(s) => {
                let _ = s.load().and_then(|_| s.deleted()).map(|_| ());
                let _ = s.backup(&target);
                true
            }
            Err(_) => false,
        });
        match outcome {
            Ok(true) => opened += 1,
            Ok(false) => refused += 1,
            Err(_) => panic!("round {round}: crashed on damage kind {how}"),
        }
    }
    println!("damaged files: {opened} opened, {refused} refused");
    let _ = std::fs::remove_dir_all(&target);
    std::fs::remove_dir_all(&dir).unwrap();
}

// ---- Round 7: the tax CSV, read back by a strict parser ----

/// RFC 4180 parser, independent of the export code: quoted fields may hold commas, quotes
/// ("") and line breaks; anything malformed is an error.
fn parse_csv_strict(text: &str) -> Result<Vec<Vec<String>>, String> {
    let mut records = Vec::new();
    let mut record = Vec::new();
    let mut field = String::new();
    let mut chars = text.chars().peekable();
    let mut at_field_start = true;
    while let Some(ch) = chars.next() {
        match ch {
            '"' if at_field_start => {
                loop {
                    match chars.next() {
                        Some('"') if chars.peek() == Some(&'"') => {
                            chars.next();
                            field.push('"');
                        }
                        Some('"') => break,
                        Some(c) => field.push(c),
                        None => return Err("unterminated quote".into()),
                    }
                }
                if !matches!(chars.peek(), Some(',') | Some('\n') | None) {
                    return Err(format!("text after a closing quote: {field:?}"));
                }
                at_field_start = false;
            }
            '"' => return Err(format!("stray quote in {field:?}")),
            ',' => {
                record.push(std::mem::take(&mut field));
                at_field_start = true;
            }
            '\n' => {
                record.push(std::mem::take(&mut field));
                records.push(std::mem::take(&mut record));
                at_field_start = true;
            }
            '\r' => return Err("bare carriage return outside quotes".into()),
            c => {
                field.push(c);
                at_field_start = false;
            }
        }
    }
    if !record.is_empty() || !field.is_empty() {
        return Err("last line has no line break".into());
    }
    Ok(records)
}

#[test]
fn tax_csv_reads_back_exactly_and_matches_the_taxes_page() {
    let texts = [
        "", "plain", "a,b", "say \"hi\"", "line1\nline2", "crlf\r\nend", "cr\ronly", "=SUM(A1)",
        "+1", "-1", "-5.50", "-", "-x", "@cmd", "\tTab", "\rCR", " =lead", "'quoted", "ﾟ✨🎰",
        "\"", "\"\"", ",", "\n", "trailing space ", "שלום", "-1,000",
    ];
    let mut rng = Rng(0xc5f);
    for round in 0..300 {
        let mut text = || {
            let a = *rng.pick(&texts);
            let b = *rng.pick(&texts);
            if rng.below(3) == 0 {
                format!("{a}{b}")
            } else {
                a.to_string()
            }
        };
        let mut data = Data::default();
        let mut entries = 0;
        let mut rng2 = Rng(round + 1);
        for id in 0..1 + rng2.below(4) as i64 {
            let date = |r: &mut Rng| {
                NaiveDate::from_ymd_opt(2023 + r.below(4) as i32, 1 + r.below(12) as u32, 1 + r.below(28) as u32)
                    .unwrap()
            };
            let mut c = Casino {
                id,
                name: format!("{}{id}", text()),
                purchases: vec![],
                redemptions: vec![],
                free_coins: vec![],
            };
            for i in 0..rng2.below(15) as i64 {
                c.purchases.push(Purchase {
                    id: i,
                    date: date(&mut rng2),
                    spent: rng2.below(1_000_000) as i64,
                    cc_received: rng2.below(1_000_000) as i64,
                    note: text(),
                });
            }
            for i in 0..rng2.below(15) as i64 {
                let amount = 1 + rng2.below(1_000_000) as i64;
                let requested = date(&mut rng2);
                c.redemptions.push(Redemption {
                    id: i,
                    date: requested,
                    amount,
                    method: *rng2.pick(&Method::ALL),
                    fee: rng2.below(amount as u64 + 1) as i64,
                    received: (rng2.below(3) > 0).then(|| requested + chrono::Days::new(rng2.below(40))),
                    note: text(),
                });
            }
            for i in 0..rng2.below(8) as i64 {
                c.free_coins.push(FreeCoins {
                    id: i,
                    date: date(&mut rng2),
                    sc: 1 + rng2.below(10_000) as i64,
                    source: *rng2.pick(&Source::ALL),
                    note: text(),
                });
            }
            entries += c.entry_count();
            data.casinos.push(c);
        }

        let out = tax::csv(&data, None);
        let records = parse_csv_strict(&out).unwrap_or_else(|e| panic!("round {round}: {e}\n{out}"));
        assert_eq!(records.len(), 1 + entries, "round {round}");
        assert!(records.iter().all(|r| r.len() == 12), "round {round}: wrong field count");
        // Every name and note comes back as written (formula-looking text gets one ').
        let mut written: Vec<String> = Vec::new();
        for c in &data.casinos {
            for note in c.purchases.iter().map(|p| &p.note)
                .chain(c.redemptions.iter().map(|r| &r.note))
                .chain(c.free_coins.iter().map(|f| &f.note))
            {
                written.push(format!("{}|{}", c.name, note));
            }
        }
        let unguard = |s: &str| s.strip_prefix('\'').map(str::to_string).unwrap_or(s.to_string());
        let mut read: Vec<String> = records[1..].iter().map(|r| format!("{}|{}", unguard(&r[1]), unguard(&r[11]))).collect();
        // A note that really starts with ' is written as-is, so unguard both sides alike.
        let mut written: Vec<String> = written.iter().map(|w| {
            let (n, note) = w.split_once('|').unwrap();
            format!("{}|{}", unguard(n), unguard(note))
        }).collect();
        written.sort();
        read.sort();
        assert_eq!(read, written, "round {round}");
        // No cell a spreadsheet would run as a formula.
        for r in &records[1..] {
            for cell in r {
                let risky = cell.starts_with(['=', '+', '@', '\t', '\r'])
                    || (cell.starts_with('-') && parse_cents(cell).is_none());
                assert!(!risky, "round {round}: formula-like cell {cell:?}");
            }
        }

        // Per-year exports add up to the Taxes page, and together they're the full export.
        let mut rows_by_year = 0;
        for year in tax::years(&data, 2026) {
            let s = TaxSummary::for_year(&data, year);
            let recs = parse_csv_strict(&tax::csv(&data, Some(year))).unwrap();
            rows_by_year += recs.len() - 1;
            let cents = |t: &str| parse_cents(t).unwrap();
            let sum = |kind: &str, col: usize, received_only: bool| -> i64 {
                recs[1..].iter()
                    .filter(|r| r[0] == kind && (!received_only || r[4] == "Received"))
                    .map(|r| cents(&r[col]))
                    .sum()
            };
            assert_eq!(sum("Purchase", 5, false), s.purchases, "round {round} {year} purchases");
            assert_eq!(sum("Redemption", 5, true), s.redeemed, "round {round} {year} redeemed");
            assert_eq!(sum("Redemption", 9, true), s.fees, "round {round} {year} fees");
            assert_eq!(sum("Free SC", 7, false), s.free_sc, "round {round} {year} free SC");
        }
        assert_eq!(rows_by_year, entries, "round {round}: per-year exports miss or repeat rows");
    }
}
