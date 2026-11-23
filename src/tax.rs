//! Tax-year summary and CSV export. Estimates to hand to a tax preparer, not tax advice.
//!
//! - A redemption counts in the year it was *received*; pending ones are listed separately.
//! - Purchases count in the year they were made.
//! - Deductible losses (only if itemizing) can't exceed winnings, and from tax year 2026
//!   only 90% of losses are deductible (2025 tax law).
//! - Filed as a professional gambler (Schedule C), only the net profit adds to AGI, but it
//!   owes self-employment tax; as a casual player every redemption adds to AGI.

use crate::model::{parse_cents, Cents, Data, Method};
use chrono::{Datelike, NaiveDate};

#[derive(Debug, Default, PartialEq)]
pub struct CasinoTax {
    pub name: String,
    pub purchases: Cents,
    pub redeemed: Cents,
    pub fees: Cents,
}

impl CasinoTax {
    pub fn net(&self) -> Cents {
        self.redeemed - self.fees - self.purchases
    }
}

#[derive(Debug, PartialEq)]
pub struct Pending {
    pub casino: String,
    pub date: NaiveDate,
    pub amount: Cents,
    pub method: Method,
}

#[derive(Debug, Default, PartialEq)]
pub struct TaxSummary {
    pub year: i32,
    /// Redemptions received during the year (before fees).
    pub redeemed: Cents,
    pub fees: Cents,
    /// Received redemptions by payout method, in `Method::ALL` order.
    pub by_method: [Cents; 3],
    pub purchases: Cents,
    pub free_sc: Cents,
    /// Every redemption still waiting to be received (any year).
    pub pending: Vec<Pending>,
    /// Casinos with activity in the year, biggest net first.
    pub casinos: Vec<CasinoTax>,
}

impl TaxSummary {
    pub fn for_year(data: &Data, year: i32) -> TaxSummary {
        let mut s = TaxSummary {
            year,
            ..Default::default()
        };
        for c in &data.casinos {
            let mut row = CasinoTax {
                name: c.name.clone(),
                ..Default::default()
            };
            for p in c.purchases.iter().filter(|p| p.date.year() == year) {
                row.purchases += p.spent;
            }
            for r in &c.redemptions {
                match r.received {
                    Some(received) if received.year() == year => {
                        row.redeemed += r.amount;
                        row.fees += r.fee;
                        let i = Method::ALL.iter().position(|m| *m == r.method).unwrap_or(0);
                        s.by_method[i] += r.amount;
                    }
                    Some(_) => {}
                    None => s.pending.push(Pending {
                        casino: c.name.clone(),
                        date: r.date,
                        amount: r.amount,
                        method: r.method,
                    }),
                }
            }
            s.free_sc += c
                .free_coins
                .iter()
                .filter(|f| f.date.year() == year)
                .map(|f| f.sc)
                .sum::<Cents>();
            s.purchases += row.purchases;
            s.redeemed += row.redeemed;
            s.fees += row.fees;
            if row.purchases != 0 || row.redeemed != 0 {
                s.casinos.push(row);
            }
        }
        s.casinos.sort_by(|a, b| b.net().cmp(&a.net()));
        s.pending.sort_by_key(|p| p.date);
        s
    }

    pub fn net(&self) -> Cents {
        self.redeemed - self.fees - self.purchases
    }

    /// Share of losses that's deductible: 90% from tax year 2026, 100% before.
    pub fn loss_percent(&self) -> i64 {
        if self.year >= 2026 {
            90
        } else {
            100
        }
    }

    /// Estimated deductible gambling losses if itemizing: purchases times `loss_percent`,
    /// never more than the year's redemptions.
    pub fn deductible_losses(&self) -> Cents {
        (self.purchases * self.loss_percent() / 100).min(self.redeemed)
    }

    /// What a casual player adds to AGI: every redemption received. Losses only come off
    /// later, as an itemized deduction.
    pub fn casual_agi(&self) -> Cents {
        self.redeemed
    }

    /// Schedule C net profit for a professional gambler: redemptions minus purchases and
    /// fees (at `loss_percent`), never below zero since losses are capped at winnings.
    pub fn schedule_c_profit(&self) -> Cents {
        let costs = (self.purchases + self.fees) * self.loss_percent() / 100;
        self.redeemed - costs.min(self.redeemed)
    }

    /// Estimated self-employment tax on the Schedule C profit: 15.3% of 92.35% of it, none
    /// under $400. Ignores the Social Security wage cap (far above typical profits).
    pub fn self_employment_tax(&self) -> Cents {
        let profit = self.schedule_c_profit();
        if profit < 40_000 {
            return 0;
        }
        (profit as f64 * 0.9235 * 0.153).round() as Cents
    }

    /// What Schedule C adds to AGI: the profit, less the deductible half of the SE tax.
    pub fn schedule_c_agi(&self) -> Cents {
        self.schedule_c_profit() - self.self_employment_tax() / 2
    }
}

/// Years with any activity (newest first), always including `this_year`.
pub fn years(data: &Data, this_year: i32) -> Vec<i32> {
    let mut years = vec![this_year];
    for c in &data.casinos {
        years.extend(c.purchases.iter().map(|p| p.date.year()));
        years.extend(
            c.redemptions
                .iter()
                .map(|r| r.received.unwrap_or(r.date).year()),
        );
        years.extend(c.free_coins.iter().map(|f| f.date.year()));
    }
    years.sort_unstable_by(|a, b| b.cmp(a));
    years.dedup();
    years
}

/// "85.50" / "-12.00": plain number for spreadsheets (no "$" or separators).
fn plain(cents: Cents) -> String {
    let sign = if cents < 0 { "-" } else { "" };
    format!(
        "{sign}{}.{:02}",
        cents.unsigned_abs() / 100,
        cents.unsigned_abs() % 100
    )
}

fn csv_field(s: &str) -> String {
    // Spreadsheet apps run cells starting with = + - @ (or a tab / carriage return) as
    // formulas. Text like that (notes, casino names) gets a leading ' so it stays text;
    // plain negative numbers are left alone.
    let looks_like_formula = s.starts_with(['=', '+', '@', '\t', '\r'])
        || (s.starts_with('-') && parse_cents(s).is_none());
    let owned;
    let s = if looks_like_formula {
        owned = format!("'{s}");
        owned.as_str()
    } else {
        s
    };
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

/// Every purchase, redemption and free-coin entry as CSV, oldest first. With `year`, only
/// that year's entries (redemptions by received date, or request date while pending).
pub fn csv(data: &Data, year: Option<i32>) -> String {
    let in_year = |d: NaiveDate| year.is_none_or(|y| d.year() == y);
    let date = |d: NaiveDate| d.format("%Y-%m-%d").to_string();
    // (sort date, fields)
    let mut rows: Vec<(NaiveDate, [String; 12])> = Vec::new();
    for c in &data.casinos {
        for p in c.purchases.iter().filter(|p| in_year(p.date)) {
            rows.push((
                p.date,
                [
                    "Purchase".into(),
                    c.name.clone(),
                    date(p.date),
                    String::new(),
                    String::new(),
                    plain(p.spent),
                    plain(p.cc_received),
                    String::new(),
                    String::new(),
                    String::new(),
                    String::new(),
                    p.note.clone(),
                ],
            ));
        }
        for r in c
            .redemptions
            .iter()
            .filter(|r| in_year(r.received.unwrap_or(r.date)))
        {
            rows.push((
                r.received.unwrap_or(r.date),
                [
                    "Redemption".into(),
                    c.name.clone(),
                    date(r.date),
                    r.received.map(date).unwrap_or_default(),
                    if r.is_pending() {
                        "Pending"
                    } else {
                        "Received"
                    }
                    .into(),
                    plain(r.amount),
                    String::new(),
                    String::new(),
                    r.method.label().into(),
                    plain(r.fee),
                    String::new(),
                    r.note.clone(),
                ],
            ));
        }
        for f in c.free_coins.iter().filter(|f| in_year(f.date)) {
            rows.push((
                f.date,
                [
                    "Free SC".into(),
                    c.name.clone(),
                    date(f.date),
                    String::new(),
                    String::new(),
                    String::new(),
                    String::new(),
                    plain(f.sc),
                    String::new(),
                    String::new(),
                    f.source.label().into(),
                    f.note.clone(),
                ],
            ));
        }
    }
    rows.sort_by_key(|(d, _)| *d);
    let mut out = String::from(
        "Type,Casino,Date,Received,Status,Amount ($),CC Received,Free SC,Method,Fee ($),Source,Note\n",
    );
    for (_, fields) in rows {
        let line: Vec<String> = fields.iter().map(|f| csv_field(f)).collect();
        out.push_str(&line.join(","));
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Casino, FreeCoins, Purchase, Redemption, Source};

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    fn data() -> Data {
        let redemption = |id, date, amount, method, fee, received| Redemption {
            id,
            date,
            amount,
            method,
            fee,
            received,
            note: String::new(),
        };
        Data {
            casinos: vec![Casino {
                id: 1,
                name: "Modo".into(),
                purchases: vec![
                    Purchase {
                        id: 1,
                        date: d(2026, 1, 5),
                        spent: 10000,
                        cc_received: 12000,
                        note: "a, \"b\"".into(),
                    },
                    Purchase {
                        id: 2,
                        date: d(2025, 12, 30),
                        spent: 5000,
                        cc_received: 5000,
                        note: String::new(),
                    },
                ],
                redemptions: vec![
                    // Requested in 2025, received in 2026: counts for 2026.
                    redemption(
                        1,
                        d(2025, 12, 31),
                        30000,
                        Method::Bank,
                        0,
                        Some(d(2026, 1, 3)),
                    ),
                    redemption(
                        2,
                        d(2026, 2, 1),
                        5000,
                        Method::GiftCard,
                        100,
                        Some(d(2026, 2, 2)),
                    ),
                    redemption(3, d(2026, 3, 1), 7500, Method::Crypto, 0, None),
                ],
                free_coins: vec![FreeCoins {
                    id: 1,
                    date: d(2026, 1, 1),
                    sc: 250,
                    source: Source::Daily,
                    note: String::new(),
                }],
            }],
        }
    }

    #[test]
    fn summarizes_a_year_by_received_date() {
        let s = TaxSummary::for_year(&data(), 2026);
        assert_eq!(s.redeemed, 35000);
        assert_eq!(s.fees, 100);
        assert_eq!(s.by_method, [30000, 5000, 0]);
        assert_eq!(s.purchases, 10000);
        assert_eq!(s.free_sc, 250);
        assert_eq!(s.net(), 24900);
        assert_eq!(s.pending.len(), 1);
        assert_eq!(s.pending[0].amount, 7500);
        // 90% of $100 losses, capped at $350 winnings.
        assert_eq!(s.deductible_losses(), 9000);

        let s2025 = TaxSummary::for_year(&data(), 2025);
        assert_eq!((s2025.redeemed, s2025.purchases), (0, 5000));
        // Can't deduct more than winnings (none in 2025); 100% rule before 2026.
        assert_eq!(s2025.loss_percent(), 100);
        assert_eq!(s2025.deductible_losses(), 0);
    }

    #[test]
    fn compares_casual_and_schedule_c() {
        let s = |redeemed, purchases, fees| TaxSummary {
            year: 2026,
            redeemed,
            purchases,
            fees,
            ..Default::default()
        };
        // $60k redeemed, $50k spent: casual adds all $60k; Schedule C deducts 90% of the
        // $50k, leaving $15k profit.
        let big = s(6_000_000, 5_000_000, 0);
        assert_eq!(big.casual_agi(), 6_000_000);
        assert_eq!(big.schedule_c_profit(), 1_500_000);
        assert_eq!(big.self_employment_tax(), 211_943); // 15,000 * 0.9235 * 0.153
        assert_eq!(big.schedule_c_agi(), 1_500_000 - 105_971);
        // Fees count as costs too.
        assert_eq!(s(6_000_000, 5_000_000, 100_000).schedule_c_profit(), 1_410_000);
        // Losses can't push the profit below zero.
        let loss = s(1_000_000, 5_000_000, 0);
        assert_eq!(loss.schedule_c_profit(), 0);
        assert_eq!(loss.self_employment_tax(), 0);
        // No SE tax under $400 of profit.
        assert_eq!(s(39_999, 0, 0).self_employment_tax(), 0);
        assert!(s(40_000, 0, 0).self_employment_tax() > 0);
        // Before 2026 all costs count.
        let mut old = s(6_000_000, 5_000_000, 0);
        old.year = 2025;
        assert_eq!(old.schedule_c_profit(), 1_000_000);
    }

    #[test]
    fn lists_years_newest_first() {
        assert_eq!(years(&data(), 2026), vec![2026, 2025]);
        assert_eq!(years(&Data::default(), 2027), vec![2027]);
    }

    #[test]
    fn exports_csv() {
        let all = csv(&data(), None);
        let lines: Vec<&str> = all.lines().collect();
        assert!(lines[0].starts_with("Type,Casino,Date,Received,Status"));
        assert_eq!(lines.len(), 1 + 6);
        // Oldest first, and quotes/commas in notes are escaped.
        assert!(lines[1].starts_with("Purchase,Modo,2025-12-30"));
        assert!(all.contains(r#""a, ""b""""#));
        assert!(all.contains("Redemption,Modo,2025-12-31,2026-01-03,Received,300.00,,,Bank,0.00,,"));
        assert!(all.contains("Redemption,Modo,2026-03-01,,Pending,75.00,,,Crypto,0.00,,"));
        assert!(all.contains("Free SC,Modo,2026-01-01,,,,,2.50,,,Daily login,"));

        let only_2025 = csv(&data(), Some(2025));
        assert_eq!(only_2025.lines().count(), 1 + 1);
    }
}
