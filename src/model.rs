//! In-memory data the UI draws from (loaded from the project's SQLite database by
//! `store.rs`), plus amount parsing/formatting, spreadsheet parsing and app settings.
//!
//! Money is kept in whole cents (`Cents`, an i64) so totals are exact. Coin amounts use the
//! same unit (hundredths of a coin), so 37.5 CC is 3750.

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// Hundredths of a dollar (or of a coin, for CC amounts).
pub type Cents = i64;

/// Largest amount accepted for one entry ($1,000,000,000). Far above anything real, and low
/// enough that totals of millions of entries can't overflow.
pub const MAX_CENTS: Cents = 100_000_000_000;

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

/// How a redemption was paid out.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Method {
    #[default]
    Bank,
    GiftCard,
    Crypto,
}

impl Method {
    pub const ALL: [Method; 3] = [Method::Bank, Method::GiftCard, Method::Crypto];

    /// Value stored in the database.
    pub fn key(self) -> &'static str {
        match self {
            Method::Bank => "bank",
            Method::GiftCard => "gift_card",
            Method::Crypto => "crypto",
        }
    }

    pub fn from_key(key: &str) -> Method {
        Method::ALL
            .into_iter()
            .find(|m| m.key() == key)
            .unwrap_or_default()
    }

    pub fn label(self) -> &'static str {
        match self {
            Method::Bank => "Bank",
            Method::GiftCard => "Gift card",
            Method::Crypto => "Crypto",
        }
    }
}

/// A withdrawal / prize redemption. `date` is when it was requested; `received` is when the
/// money arrived (None while pending). For taxes, income counts in the year it's received.
#[derive(Clone, Debug, PartialEq)]
pub struct Redemption {
    pub id: i64,
    pub date: NaiveDate,
    pub amount: Cents,
    pub method: Method,
    /// Taken out of the payout by the casino or processor.
    pub fee: Cents,
    pub received: Option<NaiveDate>,
    pub note: String,
}

impl Redemption {
    pub fn is_pending(&self) -> bool {
        self.received.is_none()
    }
}

/// Where free sweeps coins came from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Source {
    #[default]
    Daily,
    Signup,
    MailIn,
    Promo,
    Other,
}

impl Source {
    pub const ALL: [Source; 5] = [
        Source::Daily,
        Source::Signup,
        Source::MailIn,
        Source::Promo,
        Source::Other,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Source::Daily => "daily",
            Source::Signup => "signup",
            Source::MailIn => "mail_in",
            Source::Promo => "promo",
            Source::Other => "other",
        }
    }

    pub fn from_key(key: &str) -> Source {
        Source::ALL
            .into_iter()
            .find(|s| s.key() == key)
            .unwrap_or_default()
    }

    pub fn label(self) -> &'static str {
        match self {
            Source::Daily => "Daily login",
            Source::Signup => "Signup",
            Source::MailIn => "Mail-in",
            Source::Promo => "Promo",
            Source::Other => "Other",
        }
    }
}

/// Free sweeps coins (no purchase): daily logins, signup bonuses, mail-in requests...
#[derive(Clone, Debug, PartialEq)]
pub struct FreeCoins {
    pub id: i64,
    pub date: NaiveDate,
    /// Hundredths of a coin.
    pub sc: Cents,
    pub source: Source,
    pub note: String,
}

#[derive(Clone, Debug)]
pub struct Casino {
    pub id: i64,
    pub name: String,
    pub purchases: Vec<Purchase>,
    pub redemptions: Vec<Redemption>,
    pub free_coins: Vec<FreeCoins>,
}

impl Casino {
    pub fn stats(&self) -> Stats {
        let mut s = Stats::default();
        s.add(self);
        s
    }

    pub fn entry_count(&self) -> usize {
        self.purchases.len() + self.redemptions.len() + self.free_coins.len()
    }

    /// Days with a daily login bonus logged.
    pub fn daily_days(&self) -> BTreeSet<NaiveDate> {
        self.free_coins
            .iter()
            .filter(|f| f.source == Source::Daily)
            .map(|f| f.date)
            .collect()
    }

    /// Whether dailies are claimed here at all (any daily login bonus logged).
    pub fn tracks_dailies(&self) -> bool {
        self.free_coins.iter().any(|f| f.source == Source::Daily)
    }

    /// The amount of the most recent daily login bonus, which Claim logs again.
    pub fn last_daily_sc(&self) -> Option<Cents> {
        self.free_coins
            .iter()
            .filter(|f| f.source == Source::Daily && f.sc > 0)
            .max_by_key(|f| (f.date, f.id))
            .map(|f| f.sc)
    }
}

/// Consecutive days with a daily claimed, ending today. A streak isn't broken until a whole
/// day is missed, so one that ran through yesterday still counts while today is unclaimed.
pub fn streak(days: &BTreeSet<NaiveDate>, today: NaiveDate) -> u32 {
    let mut day = if days.contains(&today) {
        today
    } else {
        match today.pred_opt() {
            Some(d) => d,
            None => return 0,
        }
    };
    let mut count = 0;
    while days.contains(&day) {
        count += 1;
        match day.pred_opt() {
            Some(d) => day = d,
            None => break,
        }
    }
    count
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

    /// Days with a daily claimed at any casino.
    pub fn daily_days(&self) -> BTreeSet<NaiveDate> {
        self.casinos.iter().flat_map(|c| c.daily_days()).collect()
    }

    /// Net (redeemed - spent) per day across all casinos, for the profit chart.
    pub fn daily_net(&self) -> BTreeMap<NaiveDate, Cents> {
        let mut by_day = BTreeMap::new();
        for c in &self.casinos {
            for p in &c.purchases {
                *by_day.entry(p.date).or_default() -= p.spent;
            }
            for r in &c.redemptions {
                *by_day.entry(r.date).or_default() += r.amount - r.fee;
            }
        }
        by_day
    }
}

#[derive(Default, Clone, Copy, Debug, PartialEq)]
pub struct Stats {
    pub spent: Cents,
    pub cc_received: Cents,
    /// All redemptions, received or not.
    pub withdrawn: Cents,
    pub fees: Cents,
    /// Redemptions not received yet (included in `withdrawn`).
    pub pending: Cents,
    pub free_sc: Cents,
}

impl Stats {
    fn add(&mut self, c: &Casino) {
        for p in &c.purchases {
            self.spent += p.spent;
            self.cc_received += p.cc_received;
        }
        for r in &c.redemptions {
            self.withdrawn += r.amount;
            self.fees += r.fee;
            if r.is_pending() {
                self.pending += r.amount;
            }
        }
        for f in &c.free_coins {
            self.free_sc += f.sc;
        }
    }

    /// Money out (after fees) minus money in.
    pub fn net(&self) -> Cents {
        self.withdrawn - self.fees - self.spent
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
    if cents > MAX_CENTS {
        return None;
    }
    Some(if negative { -cents } else { cents })
}

pub fn parse_date(text: &str) -> Option<NaiveDate> {
    let text = text.trim();
    // Two-digit years first: %Y would read "3/18/26" as the year 26. (%y refuses "2026",
    // so four-digit years still fall through to %Y.)
    ["%m/%d/%y", "%m/%d/%Y", "%Y-%m-%d"]
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
            let cols = split_row(line);
            let date = parse_date(cols.first()?)?;
            // Negative amounts aren't meaningful here; treat them like blanks so the
            // preview and the import agree.
            let num = |i: usize| {
                cols.get(i)
                    .and_then(|s| parse_cents(s))
                    .filter(|v| *v >= 0)
                    .unwrap_or(0)
            };
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

/// Splits one pasted line into trimmed cells: tab separated if it has a tab, otherwise CSV,
/// where a quoted cell can hold commas ("1,000") and "" is a literal quote.
fn split_row(line: &str) -> Vec<String> {
    if line.contains('\t') {
        return line.split('\t').map(|s| s.trim().to_string()).collect();
    }
    let mut cells = vec![String::new()];
    let mut quoted = false;
    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '"' if quoted && chars.peek() == Some(&'"') => {
                chars.next();
                cells.last_mut().unwrap().push('"');
            }
            '"' => quoted = !quoted,
            ',' if !quoted => cells.push(String::new()),
            _ => cells.last_mut().unwrap().push(ch),
        }
    }
    cells.iter().map(|s| s.trim().to_string()).collect()
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
    let s = s.trim_end_matches(".0");
    // A tiny negative (e.g. a 0.01% markup) rounds to "-0".
    format!("{}%", if s == "-0" { "0" } else { s })
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

/// Writes `path` via a temp file + rename, so neither a crash nor another window saving
/// the same file at the same moment can leave it half-written. The temp name is unique to
/// this process and call: with a shared one, two writers would write into the same temp
/// file, and one could rename it into place while the other was still writing.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let tmp = path.with_file_name(format!(".{name}.{}-{n}.tmp", std::process::id()));
    fs::write(&tmp, bytes)
        .and_then(|_| fs::rename(&tmp, path))
        .inspect_err(|_| {
            let _ = fs::remove_file(&tmp);
        })
}

/// App settings: recently opened project folders (most recent first) and SweepsGrail
/// casinos hidden from the "Add casino" list.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Config {
    pub recent: Vec<PathBuf>,
    /// SweepsGrail slugs. Missing in older config files, hence the default.
    #[serde(default)]
    pub hidden_casinos: Vec<String>,
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
        write_atomic(&Self::path(), json.as_bytes()).map_err(|e| e.to_string())
    }

    pub fn push_recent(&mut self, dir: &Path) {
        self.recent.retain(|d| d != dir);
        self.recent.insert(0, dir.to_path_buf());
        self.recent.truncate(Self::MAX_RECENT);
    }

    /// Changes the saved settings: re-reads them first, so a change made meanwhile by
    /// another window isn't overwritten with this window's older copy, then saves and
    /// keeps the result.
    pub fn update(&mut self, change: impl FnOnce(&mut Config)) -> Result<(), String> {
        let mut fresh = Config::load();
        change(&mut fresh);
        *self = fresh;
        self.save()
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
pub fn legacy_cents(v: f64) -> Result<Cents, String> {
    let cents = (v * 100.0).round();
    if !cents.is_finite() || cents.abs() > MAX_CENTS as f64 {
        return Err(format!(
            "An amount in the old project file is out of range: {v}"
        ));
    }
    Ok(cents as Cents)
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
                method: Method::Bank,
                fee: 500,
                received: None,
                note: String::new(),
            }],
            free_coins: vec![FreeCoins {
                id: 1,
                date: d,
                sc: 250,
                source: Source::Daily,
                note: String::new(),
            }],
        };
        assert_eq!(casino.purchases[0].discount(), Some(0.15));
        let s = casino.stats();
        assert_eq!(s.net(), 19000); // 280 - 5 fee - 85
        assert_eq!(s.pending, 28000);
        assert_eq!(s.free_sc, 250);
        assert_eq!(s.bonus_coins(), 1500);
        let data = Data {
            casinos: vec![casino],
        };
        assert_eq!(data.daily_net()[&d], 19000);
    }

    #[test]
    fn counts_streaks() {
        let today = NaiveDate::from_ymd_opt(2026, 3, 1).unwrap(); // after Feb 28
        let day = |n: i64| today - chrono::Duration::days(n);
        let set = |ns: &[i64]| ns.iter().map(|&n| day(n)).collect::<BTreeSet<_>>();
        assert_eq!(streak(&set(&[]), today), 0);
        assert_eq!(streak(&set(&[0]), today), 1);
        assert_eq!(streak(&set(&[0, 1, 2]), today), 3);
        // Today not claimed yet: yesterday's streak is still alive.
        assert_eq!(streak(&set(&[1, 2]), today), 2);
        // A missed day ends it.
        assert_eq!(streak(&set(&[2, 3]), today), 0);
        assert_eq!(streak(&set(&[0, 2, 3]), today), 1);
        // Future-dated entries don't count.
        assert_eq!(streak(&set(&[-1]), today), 0);
    }

    #[test]
    fn daily_helpers() {
        let d = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
        let free = |id, date, sc, source| FreeCoins {
            id,
            date,
            sc,
            source,
            note: String::new(),
        };
        let mut casino = Casino {
            id: 1,
            name: "A".into(),
            purchases: vec![],
            redemptions: vec![],
            free_coins: vec![free(1, d, 500, Source::Promo)],
        };
        assert!(!casino.tracks_dailies());
        assert_eq!(casino.last_daily_sc(), None);
        let next = d.succ_opt().unwrap();
        casino.free_coins.push(free(2, next, 30, Source::Daily));
        casino.free_coins.push(free(3, d, 100, Source::Daily));
        assert!(casino.tracks_dailies());
        // Latest by date, not by entry order.
        assert_eq!(casino.last_daily_sc(), Some(30));
        assert_eq!(casino.daily_days().len(), 2);
    }

    #[test]
    fn reads_config_saved_before_hidden_casinos() {
        let old: Config = serde_json::from_str(r#"{"recent": ["/a"]}"#).unwrap();
        assert_eq!(old.recent, vec![PathBuf::from("/a")]);
        assert!(old.hidden_casinos.is_empty());
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
