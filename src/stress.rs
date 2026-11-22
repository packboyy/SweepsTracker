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
