//! In-memory data the UI draws from (loaded from the project's SQLite database by
//! `store.rs`), plus amount parsing/formatting, spreadsheet parsing and app settings.
//!
//! Money is kept in whole cents (`Cents`, an i64) so totals are exact. Coin amounts use the
//! same unit (hundredths of a coin), so 37.5 CC is 3750.

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Hundredths of a dollar (or of a coin, for CC amounts).
pub type Cents = i64;

#[derive(Clone, Debug, PartialEq)]
pub struct Purchase {
    pub id: i64,
    pub date: NaiveDate,
    pub spent: Cents,
    pub cc_received: Cents,
    pub note: String,
}

impl Purchase {
    /// e.g. spent $85 for 100 CC -> 15% discount
    pub fn discount(&self) -> Option<f64> {
        (self.spent > 0 && self.cc_received > 0)
            .then(|| (self.cc_received - self.spent) as f64 / self.cc_received as f64)
    }
}

/// A withdrawal / prize redemption.
#[derive(Clone, Debug, PartialEq)]
pub struct Redemption {
    pub id: i64,
    pub date: NaiveDate,
    pub amount: Cents,
    pub note: String,
}

#[derive(Clone, Debug)]
pub struct Casino {
    pub id: i64,
    pub name: String,
    pub purchases: Vec<Purchase>,
    pub redemptions: Vec<Redemption>,
}

impl Casino {
    pub fn stats(&self) -> Stats {
        let mut s = Stats::default();
        s.add(self);
        s
    }

    pub fn entry_count(&self) -> usize {
        self.purchases.len() + self.redemptions.len()
    }
}

/// Everything in the open project (casinos sorted by name, deleted rows excluded).
#[derive(Debug, Default)]
pub struct Data {
    pub casinos: Vec<Casino>,
}

impl Data {
    pub fn casino(&self, id: i64) -> Option<&Casino> {
        self.casinos.iter().find(|c| c.id == id)
    }

    pub fn has_casino_named(&self, name: &str) -> bool {
        self.casinos
            .iter()
            .any(|c| c.name.eq_ignore_ascii_case(name))
    }

    pub fn stats(&self) -> Stats {
        let mut s = Stats::default();
        for c in &self.casinos {
            s.add(c);
        }
        s
    }

    /// Net (redeemed - spent) per day across all casinos, for the profit chart.
    pub fn daily_net(&self) -> BTreeMap<NaiveDate, Cents> {
        let mut by_day = BTreeMap::new();
        for c in &self.casinos {
            for p in &c.purchases {
                *by_day.entry(p.date).or_default() -= p.spent;
            }
            for r in &c.redemptions {
                *by_day.entry(r.date).or_default() += r.amount;
            }
        }
        by_day
    }
}

#[derive(Default, Clone, Copy, Debug, PartialEq)]
pub struct Stats {
    pub spent: Cents,
    pub cc_received: Cents,
    pub withdrawn: Cents,
}

impl Stats {
    fn add(&mut self, c: &Casino) {
        for p in &c.purchases {
            self.spent += p.spent;
            self.cc_received += p.cc_received;
        }
        for r in &c.redemptions {
            self.withdrawn += r.amount;
        }
    }

    /// Money out minus money in.
    pub fn net(&self) -> Cents {
        self.withdrawn - self.spent
    }

    /// Extra coins gained from buying at a discount.
    pub fn bonus_coins(&self) -> Cents {
        self.cc_received - self.spent
    }

    pub fn avg_discount(&self) -> Option<f64> {
        (self.cc_received > 0).then(|| self.bonus_coins() as f64 / self.cc_received as f64)
    }
}

/// Parses "85", "$1,000", "37.5", "8.333" (rounded to the cent) into cents without going
/// through floating point. Empty or invalid -> None.
pub fn parse_cents(text: &str) -> Option<Cents> {
    let cleaned: String = text
        .chars()
        .filter(|c| !matches!(c, '$' | ',' | ' ' | '"'))
        .collect();
    let (negative, digits) = match cleaned.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, cleaned.as_str()),
    };
    let (whole, frac) = digits.split_once('.').unwrap_or((digits, ""));
    let all_digits = |s: &str| s.chars().all(|c| c.is_ascii_digit());
    if (whole.is_empty() && frac.is_empty()) || !all_digits(whole) || !all_digits(frac) {
        return None;
    }
    let whole: i64 = if whole.is_empty() {
        0
    } else {
        whole.parse().ok()?
    };
    let frac_digits: Vec<i64> = frac.bytes().map(|b| (b - b'0') as i64).collect();
    let tenths = frac_digits.first().copied().unwrap_or(0);
    let hundredths = frac_digits.get(1).copied().unwrap_or(0);
    let round_up = frac_digits.get(2).is_some_and(|d| *d >= 5) as i64;
    let cents = whole
        .checked_mul(100)?
        .checked_add(tenths * 10 + hundredths + round_up)?;
    Some(if negative { -cents } else { cents })
}

pub fn parse_date(text: &str) -> Option<NaiveDate> {
    let text = text.trim();
    ["%m/%d/%Y", "%m/%d/%y", "%Y-%m-%d"]
        .iter()
        .find_map(|fmt| NaiveDate::parse_from_str(text, fmt).ok())
}

/// One pasted spreadsheet row. A row can hold a purchase (spent / CC), a withdrawal, or both.
#[derive(Debug, PartialEq)]
pub struct ParsedRow {
    pub date: NaiveDate,
    pub spent: Cents,
    pub cc_received: Cents,
    pub withdrawal: Cents,
}

/// Parses rows copied from Excel/Sheets (tab separated) or CSV in the order
/// DATE, SPENT, CC RECEIVED, WITHDRAWAL. Header and blank rows are skipped.
pub fn parse_spreadsheet(text: &str) -> Vec<ParsedRow> {
    text.lines()
        .filter_map(|line| {
            let sep = if line.contains('\t') { '\t' } else { ',' };
            let cols: Vec<&str> = line.split(sep).map(str::trim).collect();
            let date = parse_date(cols.first()?)?;
            let num = |i: usize| cols.get(i).and_then(|s| parse_cents(s)).unwrap_or(0);
            let row = ParsedRow {
                date,
                spent: num(1),
                cc_received: num(2),
                withdrawal: num(3),
            };
            (row.spent != 0 || row.cc_received != 0 || row.withdrawal != 0).then_some(row)
        })
        .collect()
}

/// "$1,234.50" / "-$90.00"
pub fn money(cents: Cents) -> String {
    let abs = cents.unsigned_abs();
    let dollars = (abs / 100).to_string();
    let mut grouped = String::new();
    for (i, ch) in dollars.chars().enumerate() {
        if i > 0 && (dollars.len() - i) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(ch);
    }
    let sign = if cents < 0 { "-" } else { "" };
    format!("{sign}${grouped}.{:02}", abs % 100)
}

pub fn signed_money(cents: Cents) -> String {
    if cents > 0 {
        format!("+{}", money(cents))
    } else {
        money(cents)
    }
}

/// Coin amount without trailing zeros: 3750 -> "37.5", 10000 -> "100".
pub fn coins(hundredths: Cents) -> String {
    let sign = if hundredths < 0 { "-" } else { "" };
    let abs = hundredths.unsigned_abs();
    let s = format!("{sign}{}.{:02}", abs / 100, abs % 100);
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// Plain number for an input box (no "$" or thousands separators): 8550 -> "85.5".
pub fn edit_text(cents: Cents) -> String {
    if cents == 0 {
        String::new()
    } else {
        coins(cents)
    }
}

pub fn percent(v: f64) -> String {
    let s = format!("{:.1}", v * 100.0);
    format!("{}%", s.trim_end_matches(".0"))
}

// ---- Projects and settings ------------------------------------------------------------

/// App-wide folder: recent projects list and the shared SweepsGrail cache.
/// (Projects themselves live wherever the user puts them.)
pub fn app_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("SweepsTracker")
}

pub fn project_name(dir: &Path) -> String {
    dir.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| dir.display().to_string())
}

/// Remembers recently opened project folders (most recent first).
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Config {
    pub recent: Vec<PathBuf>,
}

impl Config {
    const MAX_RECENT: usize = 10;

    fn path() -> PathBuf {
        app_dir().join("config.json")
    }

    pub fn load() -> Self {
        fs::read_to_string(Self::path())
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> Result<(), String> {
        fs::create_dir_all(app_dir()).map_err(|e| e.to_string())?;
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        fs::write(Self::path(), json).map_err(|e| e.to_string())
    }

    pub fn push_recent(&mut self, dir: &Path) {
        self.recent.retain(|d| d != dir);
        self.recent.insert(0, dir.to_path_buf());
        self.recent.truncate(Self::MAX_RECENT);
    }
}

// ---- Old JSON format (import only) ----------------------------------------------------

/// Project file used before the SQLite database; imported once by `store.rs`.
pub const LEGACY_PROJECT_FILE: &str = "sweeps.json";

#[derive(Debug, Deserialize)]
pub struct LegacyData {
    pub casinos: Vec<LegacyCasino>,
}

#[derive(Debug, Deserialize)]
pub struct LegacyCasino {
    pub name: String,
    pub entries: Vec<LegacyEntry>,
}

/// Old combined row: a purchase and/or a withdrawal, amounts as floating-point dollars.
#[derive(Debug, Deserialize)]
pub struct LegacyEntry {
    pub date: NaiveDate,
    pub spent: f64,
    pub cc_received: f64,
    pub withdrawal: f64,
    #[serde(default)]
    pub note: String,
}

/// Float dollars from the old format -> cents.
pub fn legacy_cents(v: f64) -> Cents {
    (v * 100.0).round() as Cents
}

/// Before projects existed, data lived in the app folder as data.json. On first run of the
/// project-based version, copy it into ~/Documents/Sweeps Tracker (the original is left in
/// place) and return that folder; opening it then imports the JSON into the database.
pub fn migrate_legacy_data() -> Option<PathBuf> {
    let legacy = app_dir().join("data.json");
    if !legacy.exists() {
        return None;
    }
    let dir = dirs::document_dir()?.join("Sweeps Tracker");
    let target = dir.join(LEGACY_PROJECT_FILE);
    let already_migrated = target.exists() || crate::store::has_database(&dir);
    if !already_migrated {
        fs::create_dir_all(&dir).ok()?;
        fs::copy(&legacy, &target).ok()?;
    }
    Some(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_cents_exactly() {
        assert_eq!(parse_cents("85"), Some(8500));
        assert_eq!(parse_cents("37.5"), Some(3750));
        assert_eq!(parse_cents("$1,234.56"), Some(123456));
        assert_eq!(parse_cents(".5"), Some(50));
        assert_eq!(parse_cents("8.333"), Some(833));
        assert_eq!(parse_cents("8.335"), Some(834));
        assert_eq!(parse_cents("0.1"), Some(10));
        assert_eq!(parse_cents("-90"), Some(-9000));
        assert_eq!(parse_cents(""), None);
        assert_eq!(parse_cents("abc"), None);
        assert_eq!(parse_cents("1.2.3"), None);
        // 0.1 + 0.2 is exact in cents (it isn't in f64).
        assert_eq!(
            parse_cents("0.1").unwrap() + parse_cents("0.2").unwrap(),
            30
        );
    }

    #[test]
    fn parses_pasted_excel_rows() {
        let text = "DATE\tSPENT\tCC RECEIVED\tWITHDRAWAL\n\
                    3/13/2026\t90\t100\t\n\
                    3/14/2026\t\t\t500\n\
                    3/16/2026\t37.5\t50\t\n\
                    \n";
        let rows = parse_spreadsheet(text);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].date, NaiveDate::from_ymd_opt(2026, 3, 13).unwrap());
        assert_eq!(rows[0].spent, 9000);
        assert_eq!(rows[0].cc_received, 10000);
        assert_eq!(rows[1].withdrawal, 50000);
        assert_eq!(rows[2].spent, 3750);
    }

    #[test]
    fn parses_csv_and_dollar_signs() {
        let rows = parse_spreadsheet("2026-03-15,$210,300,\n3/18/26,,,\"577\"");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].spent, 21000);
        assert_eq!(rows[1].withdrawal, 57700);
    }

    #[test]
    fn stats_and_discount() {
        let d = NaiveDate::from_ymd_opt(2026, 3, 13).unwrap();
        let casino = Casino {
            id: 1,
            name: "Modo".into(),
            purchases: vec![Purchase {
                id: 1,
                date: d,
                spent: 8500,
                cc_received: 10000,
                note: String::new(),
            }],
            redemptions: vec![Redemption {
                id: 1,
                date: d,
                amount: 28000,
                note: String::new(),
            }],
        };
        assert_eq!(casino.purchases[0].discount(), Some(0.15));
        let s = casino.stats();
        assert_eq!(s.net(), 19500);
        assert_eq!(s.bonus_coins(), 1500);
        let data = Data {
            casinos: vec![casino],
        };
        assert_eq!(data.daily_net()[&d], 19500);
    }

    #[test]
    fn formats_money() {
        assert_eq!(money(123450), "$1,234.50");
        assert_eq!(money(-9000), "-$90.00");
        assert_eq!(money(5), "$0.05");
        assert_eq!(signed_money(850), "+$8.50");
        assert_eq!(percent(0.15), "15%");
        assert_eq!(percent(0.125), "12.5%");
        assert_eq!(coins(3750), "37.5");
        assert_eq!(coins(10000), "100");
        assert_eq!(edit_text(8550), "85.5");
        assert_eq!(edit_text(0), "");
    }
}
