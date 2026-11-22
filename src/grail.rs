//! Casino directory scraped from the SweepsGrail "Live Board" (https://sweepsgrail.com/).
//! The table is server-rendered: one `<tr data-tier>` per casino with 17 `<td>` columns:
//! Name | NEW | Signup | Tier | RedeemSpeed | Votes | WelcomeBonus | DailyFreebie | PromoValue |
//! CashRedeem | GiftRedeem | CryptoRedeem | VIP | Notes | Parent | Located | Review
//! Only the homepage is fetched; robots.txt allows it (it disallows /go/ and /api/).

use eframe::egui;
use scraper::{ElementRef, Html, Selector};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};

pub const SITE: &str = "https://sweepsgrail.com";
const USER_AGENT: &str = "SweepsTracker/0.1 (personal spending tracker)";
/// Refresh the cached list once a day.
const MAX_AGE_SECS: i64 = 24 * 60 * 60;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct GrailCasino {
    pub name: String,
    pub slug: String,
    pub logo: String,
    /// Community consensus tier: S, A, B or C.
    pub tier: String,
    pub is_new: bool,
    /// e.g. "Normal (next day to 3 days)"
    pub redeem_speed: String,
    /// Short form, e.g. "2.5 SC"
    pub signup_bonus: String,
    /// Full welcome offer, one line per part.
    pub welcome_bonus: String,
    pub daily_freebie: String,
    pub promo_value: String,
    pub cash_min: String,
    pub gift_min: String,
    pub crypto_min: String,
    pub vip: bool,
    pub vip_note: String,
    pub parent: String,
    pub located: String,
    pub review_url: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct GrailDb {
    /// Unix seconds of the last successful fetch.
    pub fetched_at: i64,
    pub casinos: Vec<GrailCasino>,
}

/// Short names people use for casinos listed under a longer name.
const ALIASES: &[(&str, &str)] = &[("ccc", "crowncoins")];

/// Lowercase letters and digits only, so "Jackpot Daily" matches "JackpotDaily".
pub fn normalize(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

impl GrailDb {
    pub fn path() -> PathBuf {
        crate::model::app_dir().join("sweepsgrail.json")
    }

    /// The saved copy from the last download, or, if there isn't a usable one (fresh install,
    /// damaged file), the list built into the app. Either way a refresh is attempted once
    /// it's more than a day old.
    pub fn load(path: &Path) -> Self {
        fs::read_to_string(path)
            .ok()
            .and_then(|t| serde_json::from_str::<GrailDb>(&t).ok())
            .filter(|db| !db.casinos.is_empty())
            .unwrap_or_else(|| GrailDb {
                // Built-in data is always out of date: refresh as soon as possible.
                fetched_at: 0,
                ..Self::bundled()
            })
    }

    /// Snapshot of the Live Board shipped with the app (assets/sweepsgrail-snapshot.json),
    /// so the casino list works offline and if the site changes in a way the parser can't read.
    pub fn bundled() -> Self {
        serde_json::from_str(include_str!("../assets/sweepsgrail-snapshot.json"))
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let json = serde_json::to_string(self).map_err(|e| e.to_string())?;
        crate::model::write_atomic(path, json.as_bytes()).map_err(|e| e.to_string())
    }

    pub fn is_stale(&self) -> bool {
        chrono::Utc::now().timestamp() - self.fetched_at > MAX_AGE_SECS
    }

    pub fn find(&self, name: &str) -> Option<&GrailCasino> {
        self.find_index(name).map(|i| &self.casinos[i])
    }

    /// Position in `casinos` of the listing matching `name` (loosely, plus aliases).
    pub fn find_index(&self, name: &str) -> Option<usize> {
        let mut key = normalize(name);
        if let Some((_, full)) = ALIASES.iter().find(|(short, _)| *short == key) {
            key = full.to_string();
        }
        self.casinos.iter().position(|c| normalize(&c.name) == key)
    }
}

fn absolute(url: &str) -> String {
    if url.starts_with('/') {
        format!("{SITE}{url}")
    } else {
        url.to_string()
    }
}

/// "-" and blanks mean "not offered".
fn clean(s: String) -> String {
    if s == "-" {
        String::new()
    } else {
        s
    }
}

/// Column positions in the Live Board table. Found by header text when possible, so a
/// reordered or added column doesn't break parsing; `FALLBACK` is the layout as of 11/2026.
#[derive(Debug, PartialEq)]
struct Columns {
    name: usize,
    tier: Option<usize>,
    speed: Option<usize>,
    welcome: Option<usize>,
    daily: Option<usize>,
    promo: Option<usize>,
    cash: Option<usize>,
    gift: Option<usize>,
    crypto: Option<usize>,
    vip: Option<usize>,
    parent: Option<usize>,
    located: Option<usize>,
}

const FALLBACK: Columns = Columns {
    name: 0,
    tier: Some(3),
    speed: Some(4),
    welcome: Some(6),
    daily: Some(7),
    promo: Some(8),
    cash: Some(9),
    gift: Some(10),
    crypto: Some(11),
    vip: Some(12),
    parent: Some(14),
    located: Some(15),
};

impl Columns {
    /// Maps header cells ("Name", "Tier", "RedeemSpeed", ...) to columns. Needs at least the
    /// Name and Tier headers; otherwise the caller falls back to `FALLBACK`.
    fn from_headers(headers: &[String]) -> Option<Columns> {
        let find = |names: &[&str]| {
            headers
                .iter()
                .position(|h| names.contains(&normalize(h).as_str()))
        };
        Some(Columns {
            name: find(&["name", "casino"])?,
            tier: Some(find(&["tier"])?),
            speed: find(&["redeemspeed", "redemptionspeed"]),
            welcome: find(&["welcomebonus", "bonus"]),
            daily: find(&["dailyfreebie", "daily"]),
            promo: find(&["promovalue", "promos"]),
            cash: find(&["cashredeem"]),
            gift: find(&["giftredeem"]),
            crypto: find(&["cryptoredeem"]),
            vip: find(&["vip"]),
            parent: find(&["parent", "operator"]),
            located: find(&["located", "location"]),
        })
    }
}

pub fn parse(html: &str) -> Vec<GrailCasino> {
    let sel = |s: &str| Selector::parse(s).expect("valid selector");
    let (row_sel, td_sel, img_sel) = (sel("tr[data-tier]"), sel("td"), sel("img"));
    let (th_sel, title_sel, new_sel) = (sel("thead th"), sel("[title]"), sel("[data-new-badge]"));
    let review_sel = sel("a[href*='/casino/']");

    let doc = Html::parse_document(html);
    let headers: Vec<String> = doc
        .select(&th_sel)
        .map(|th| th.text().collect::<String>().trim().to_string())
        .collect();
    let cols = Columns::from_headers(&headers).unwrap_or(FALLBACK);

    doc.select(&row_sel)
        .filter_map(|row| {
            let cells: Vec<ElementRef> = row.select(&td_sel).collect();
            let cell = |i: Option<usize>| i.and_then(|i| cells.get(i));
            let text = |i: Option<usize>| {
                cell(i)
                    .map(|c| clean(c.text().collect::<String>().trim().to_string()))
                    .unwrap_or_default()
            };
            let titles = |i: Option<usize>| -> Vec<String> {
                cell(i)
                    .map(|c| {
                        c.select(&title_sel)
                            .filter_map(|e| e.value().attr("title"))
                            .map(str::to_string)
                            .collect()
                    })
                    .unwrap_or_default()
            };

            // Logo, NEW badge and review link are looked up anywhere in the row.
            let img = row.select(&img_sel).next();
            let name = img
                .and_then(|i| i.value().attr("alt"))
                .map(str::to_string)
                .filter(|n| !n.is_empty())
                .unwrap_or_else(|| text(Some(cols.name)));
            if name.is_empty() {
                return None;
            }
            let review = row
                .select(&review_sel)
                .next()
                .and_then(|a| a.value().attr("href"))
                .unwrap_or_default();
            let slug = review.rsplit('/').next().unwrap_or_default().to_string();
            // Second title is "Normal (next day to 3 days) · 402 reports · ..."; keep the first part.
            let speed = titles(cols.speed)
                .get(1)
                .map(|t| t.split(" · ").next().unwrap_or_default().to_string())
                .unwrap_or_else(|| text(cols.speed));
            let signup = text(cols.welcome);
            let welcome = titles(cols.welcome)
                .into_iter()
                .next()
                .unwrap_or_else(|| signup.clone());

            Some(GrailCasino {
                logo: img
                    .and_then(|i| i.value().attr("src"))
                    .map(absolute)
                    .unwrap_or_default(),
                is_new: row.select(&new_sel).next().is_some(),
                tier: text(cols.tier),
                redeem_speed: speed,
                signup_bonus: signup,
                welcome_bonus: welcome,
                daily_freebie: text(cols.daily),
                promo_value: text(cols.promo),
                cash_min: text(cols.cash),
                gift_min: text(cols.gift),
                crypto_min: text(cols.crypto),
                vip: text(cols.vip).starts_with("Yes"),
                vip_note: titles(cols.vip).into_iter().next().unwrap_or_default(),
                parent: text(cols.parent),
                located: text(cols.located),
                review_url: if review.is_empty() {
                    String::new()
                } else {
                    absolute(review)
                },
                slug,
                name,
            })
        })
        .collect()
}

fn fetch() -> Result<GrailDb, String> {
    let html = ureq::get(SITE)
        .set("User-Agent", USER_AGENT)
        .call()
        .map_err(|e| e.to_string())?
        .into_string()
        .map_err(|e| e.to_string())?;
    let casinos = parse(&html);
    if casinos.is_empty() {
        return Err("no casinos found on the page (the site layout may have changed)".into());
    }
    Ok(GrailDb {
        fetched_at: chrono::Utc::now().timestamp(),
        casinos,
    })
}

/// Fetches in a background thread and saves the result; repaints the UI when done.
pub fn fetch_in_background(ctx: egui::Context, path: PathBuf) -> Receiver<Result<GrailDb, String>> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let result = fetch().and_then(|db| db.save(&path).map(|_| db));
        let _ = tx.send(result);
        ctx.request_repaint();
    });
    rx
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(name: &str, img: &str, new: bool, tier: &str, review: &str) -> String {
        let new_badge = if new {
            r#"<span data-new-badge="true" title="New casino">NEW</span>"#
        } else {
            ""
        };
        format!(
            r#"<tr data-tier="S">
            <td><a href="/go/x"><img src="{img}" alt="{name}"></a><a href="/go/x">{name}</a></td>
            <td>{new_badge}</td>
            <td><a href="/go/x">Play</a></td>
            <td><span title="Community consensus: {tier}">{tier}</span></td>
            <td><span title="Mixed reports">x</span><span title="Normal (next day to 3 days) · 402 reports">Normal</span></td>
            <td>12/299/49</td>
            <td><a href="/go/x" title="Welcome bonus: 120,000 GC + 60 SC&#10;Signup SC: 2.5 free SC">2.5 SC</a></td>
            <td>MED</td><td>HIGH</td><td>$75</td><td>$10</td><td>-</td>
            <td>Yes<span title="Can level up your VIP!">Can level up your VIP!</span></td>
            <td></td><td>B-Two Operations Limited (B2)</td><td>Isle Of Man</td>
            <td><a href="{review}" title="Open full review"></a></td>
            </tr>"#
        )
    }

    #[test]
    fn parses_live_board_rows() {
        let html = format!(
            "<table><thead><tr><th>Name</th></tr></thead><tbody>{}{}</tbody></table>",
            row(
                "McLuck",
                "https://wsrv.nl/?url=x&amp;w=64",
                false,
                "A",
                "/casino/mcluck"
            ),
            row(
                "Rainbet.us",
                "/api/public/img/feed/180-rainbetus.webp",
                true,
                "B",
                "/casino/rainbet"
            ),
        );
        let casinos = parse(&html);
        assert_eq!(casinos.len(), 2);
        let m = &casinos[0];
        assert_eq!(m.name, "McLuck");
        assert_eq!(m.slug, "mcluck");
        assert_eq!(m.logo, "https://wsrv.nl/?url=x&w=64");
        assert_eq!(m.tier, "A");
        assert!(!m.is_new);
        assert_eq!(m.redeem_speed, "Normal (next day to 3 days)");
        assert_eq!(m.signup_bonus, "2.5 SC");
        assert!(m.welcome_bonus.starts_with("Welcome bonus: 120,000 GC"));
        assert_eq!(
            (m.daily_freebie.as_str(), m.promo_value.as_str()),
            ("MED", "HIGH")
        );
        assert_eq!(
            (
                m.cash_min.as_str(),
                m.gift_min.as_str(),
                m.crypto_min.as_str()
            ),
            ("$75", "$10", "")
        );
        assert!(m.vip);
        assert_eq!(m.parent, "B-Two Operations Limited (B2)");
        assert_eq!(m.review_url, "https://sweepsgrail.com/casino/mcluck");
        let r = &casinos[1];
        assert!(r.is_new);
        assert_eq!(
            r.logo,
            "https://sweepsgrail.com/api/public/img/feed/180-rainbetus.webp"
        );
    }

    #[test]
    fn finds_columns_by_header_when_the_layout_changes() {
        // Columns reordered, some removed, one added: found by header text, not position.
        let html = r#"<table>
            <thead><tr><th>Tier</th><th>Extra</th><th>Name</th><th>Located</th><th>CashRedeem</th><th>Review</th></tr></thead>
            <tbody><tr data-tier="S">
                <td>A</td>
                <td>whatever</td>
                <td><img src="/img/180-modo.jpg" alt="Modo"><span data-new-badge="true">NEW</span></td>
                <td>USA - Arizona</td>
                <td>$50</td>
                <td><a href="/casino/modo">Review</a></td>
            </tr></tbody></table>"#;
        let casinos = parse(html);
        assert_eq!(casinos.len(), 1);
        let m = &casinos[0];
        assert_eq!(m.name, "Modo");
        assert_eq!(m.tier, "A");
        assert_eq!(m.located, "USA - Arizona");
        assert_eq!(m.cash_min, "$50");
        assert_eq!(m.gift_min, ""); // column no longer exists
        assert!(m.is_new);
        assert_eq!(m.slug, "modo");
        assert_eq!(m.logo, "https://sweepsgrail.com/img/180-modo.jpg");
    }

    #[test]
    fn falls_back_to_the_bundled_list() {
        let bundled = GrailDb::bundled();
        assert!(bundled.casinos.len() > 50);
        assert!(bundled.find("Modo").is_some());
        // No saved copy (fresh install) -> the bundled list, marked stale so it refreshes.
        let missing = std::env::temp_dir().join("sweeps-no-such-cache.json");
        let db = GrailDb::load(&missing);
        assert_eq!(db.casinos.len(), bundled.casinos.len());
        assert!(db.is_stale());
    }

    #[test]
    fn matches_names_loosely() {
        assert_eq!(normalize("Jackpot Daily"), normalize("JackpotDaily"));
        assert_eq!(normalize("Rainbet.us"), "rainbetus");
    }

    /// Runs against a saved copy of the real page: GRAIL_HTML=path cargo test -- --ignored
    #[test]
    #[ignore]
    fn parses_saved_homepage() {
        let path = std::env::var("GRAIL_HTML").expect("set GRAIL_HTML");
        let casinos = parse(&fs::read_to_string(path).unwrap());
        for c in &casinos {
            println!(
                "{:<22} {:<2} new={:<5} {:<28} {:<8} cash={:<5} {}",
                c.name, c.tier, c.is_new, c.redeem_speed, c.signup_bonus, c.cash_min, c.logo
            );
        }
        assert!(casinos.len() > 50, "only {} casinos", casinos.len());
    }
}
