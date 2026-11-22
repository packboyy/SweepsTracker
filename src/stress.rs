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
