//! Project database: one SQLite file (`sweeps.db`) per project folder.
//!
//! - Amounts are INTEGER cents (coins in hundredths), never floating point.
//! - Purchases, redemptions and free coins are separate tables.
//! - Every row has created_at / updated_at; deleting sets deleted_at instead of erasing,
//!   so the record history stays available for taxes (and "Recently deleted" can restore).
//! - The schema version lives in `PRAGMA user_version`; `migrate` upgrades step by step.
//! - The connection holds an exclusive lock, so a second window can't open the same project.

use crate::model::{
    legacy_cents, Casino, Cents, Data, FreeCoins, LegacyData, Method, ParsedRow, Purchase,
    Redemption, Source, LEGACY_PROJECT_FILE,
};
use chrono::NaiveDate;
use rusqlite::{params, Connection, ErrorCode};
use std::fs;
use std::path::{Path, PathBuf};

pub const DB_FILE: &str = "sweeps.db";
const BACKUP_DIR: &str = "backups";
const BACKUPS_KEPT: usize = 30;

/// Schema migrations, in order. `user_version` = number of migrations applied.
const MIGRATIONS: &[&str] = &[
    // 1: casinos, purchases, redemptions
    "
    CREATE TABLE casinos (
        id          INTEGER PRIMARY KEY,
        name        TEXT NOT NULL,
        created_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
        updated_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
        deleted_at  TEXT
    );
    -- Names are unique among casinos that haven't been deleted.
    CREATE UNIQUE INDEX casinos_name ON casinos (name COLLATE NOCASE) WHERE deleted_at IS NULL;

    CREATE TABLE purchases (
        id                 INTEGER PRIMARY KEY,
        casino_id          INTEGER NOT NULL REFERENCES casinos (id),
        date               TEXT NOT NULL,
        spent_cents        INTEGER NOT NULL CHECK (spent_cents >= 0),
        cc_received_cents  INTEGER NOT NULL CHECK (cc_received_cents >= 0),
        note               TEXT NOT NULL DEFAULT '',
        created_at         TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
        updated_at         TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
        deleted_at         TEXT,
        CHECK (spent_cents > 0 OR cc_received_cents > 0)
    );
    CREATE INDEX purchases_casino ON purchases (casino_id, date);

    CREATE TABLE redemptions (
        id            INTEGER PRIMARY KEY,
        casino_id     INTEGER NOT NULL REFERENCES casinos (id),
        date          TEXT NOT NULL,
        amount_cents  INTEGER NOT NULL CHECK (amount_cents > 0),
        note          TEXT NOT NULL DEFAULT '',
        created_at    TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
        updated_at    TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
        deleted_at    TEXT
    );
    CREATE INDEX redemptions_casino ON redemptions (casino_id, date);
    ",
    // 2: tax fields on redemptions, free coins, and tracking which entries were deleted
    //    together with their casino (so restoring the casino restores exactly those).
    "
    ALTER TABLE redemptions ADD COLUMN method TEXT NOT NULL DEFAULT 'bank'
        CHECK (method IN ('bank', 'gift_card', 'crypto'));
    ALTER TABLE redemptions ADD COLUMN fee_cents INTEGER NOT NULL DEFAULT 0
        CHECK (fee_cents >= 0);
    -- NULL while pending. Redemptions recorded before this existed count as received.
    ALTER TABLE redemptions ADD COLUMN received_date TEXT;
    UPDATE redemptions SET received_date = date;

    ALTER TABLE purchases ADD COLUMN deleted_with_casino INTEGER NOT NULL DEFAULT 0;
    ALTER TABLE redemptions ADD COLUMN deleted_with_casino INTEGER NOT NULL DEFAULT 0;
    UPDATE purchases SET deleted_with_casino = 1
        WHERE deleted_at IS NOT NULL
          AND deleted_at = (SELECT c.deleted_at FROM casinos c WHERE c.id = casino_id);
    UPDATE redemptions SET deleted_with_casino = 1
        WHERE deleted_at IS NOT NULL
          AND deleted_at = (SELECT c.deleted_at FROM casinos c WHERE c.id = casino_id);

    CREATE TABLE free_coins (
        id                   INTEGER PRIMARY KEY,
        casino_id            INTEGER NOT NULL REFERENCES casinos (id),
        date                 TEXT NOT NULL,
        sc_cents             INTEGER NOT NULL CHECK (sc_cents > 0),
        source               TEXT NOT NULL DEFAULT 'daily'
            CHECK (source IN ('daily', 'signup', 'mail_in', 'promo', 'other')),
        note                 TEXT NOT NULL DEFAULT '',
        created_at           TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
        updated_at           TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
        deleted_at           TEXT,
        deleted_with_casino  INTEGER NOT NULL DEFAULT 0
    );
    CREATE INDEX free_coins_casino ON free_coins (casino_id, date);
    ",
];

const NOW: &str = "strftime('%Y-%m-%dT%H:%M:%fZ', 'now')";

/// Tables holding a casino's entries (soft-deleted / restored together with the casino).
const ENTRY_TABLES: [&str; 3] = ["purchases", "redemptions", "free_coins"];

/// A purchase, redemption or free-coins row, by id.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EntryRef {
    Purchase(i64),
    Redemption(i64),
    FreeCoins(i64),
}

impl EntryRef {
    fn table(self) -> &'static str {
        match self {
            EntryRef::Purchase(_) => "purchases",
            EntryRef::Redemption(_) => "redemptions",
            EntryRef::FreeCoins(_) => "free_coins",
        }
    }

    fn id(self) -> i64 {
        match self {
            EntryRef::Purchase(id) | EntryRef::Redemption(id) | EntryRef::FreeCoins(id) => id,
        }
    }
}

/// Fields of a redemption as entered in the form / edit dialog.
#[derive(Clone, Debug, PartialEq)]
pub struct RedemptionInput {
    pub date: NaiveDate,
    pub amount: Cents,
    pub method: Method,
    pub fee: Cents,
    pub received: Option<NaiveDate>,
    pub note: String,
}

/// What "Recently deleted" shows.
#[derive(Debug, Default)]
pub struct Deleted {
    pub casinos: Vec<DeletedCasino>,
    pub entries: Vec<DeletedEntry>,
}

#[derive(Debug)]
pub struct DeletedCasino {
    pub id: i64,
    pub name: String,
    /// UTC timestamp, e.g. 2026-11-22T03:40:12.123Z
    pub deleted_at: String,
    /// Entries deleted along with it.
    pub entries: i64,
}

#[derive(Debug)]
pub struct DeletedEntry {
    pub entry: EntryRef,
    pub casino: String,
    pub date: NaiveDate,
    /// Spent (purchase), amount (redemption) or SC (free coins).
    pub amount: Cents,
    pub deleted_at: String,
}

pub fn db_path(dir: &Path) -> PathBuf {
    dir.join(DB_FILE)
}

pub fn has_database(dir: &Path) -> bool {
    db_path(dir).exists()
}

/// A folder counts as a project if it has a database or an old JSON file to import.
pub fn is_project(dir: &Path) -> bool {
    has_database(dir) || dir.join(LEGACY_PROJECT_FILE).exists()
}

pub struct Store {
    conn: Connection,
}

fn err(e: rusqlite::Error) -> String {
    match e.sqlite_error_code() {
        Some(ErrorCode::DatabaseBusy) | Some(ErrorCode::DatabaseLocked) => {
            "This project is already open in another Sweeps Tracker window.".into()
        }
        Some(ErrorCode::ConstraintViolation) if e.to_string().contains("casinos") => {
            "You already have a casino with that name.".into()
        }
        _ => e.to_string(),
    }
}

impl Store {
    /// Opens (creating if needed) the project database in `dir`, takes the project lock,
    /// applies migrations, and imports an old sweeps.json if this is a new database.
    pub fn open(dir: &Path) -> Result<Store, String> {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        let is_new = !has_database(dir);
        let conn = Connection::open(db_path(dir)).map_err(err)?;
        // Fail fast (instead of the default 5 s wait) if another window holds the lock.
        conn.busy_timeout(std::time::Duration::from_millis(250))
            .map_err(err)?;
        conn.execute_batch(
            "PRAGMA foreign_keys = ON;
             PRAGMA synchronous = FULL;
             PRAGMA locking_mode = EXCLUSIVE;",
        )
        .map_err(err)?;
        // Take the exclusive lock now (it's then held until the connection closes).
        conn.execute_batch("BEGIN EXCLUSIVE; COMMIT;")
            .map_err(err)?;
        let mut store = Store { conn };
        store.migrate()?;
        let legacy = dir.join(LEGACY_PROJECT_FILE);
        if is_new && legacy.exists() {
            store.import_legacy_json(&legacy)?;
            let _ = fs::rename(&legacy, dir.join(format!("{LEGACY_PROJECT_FILE}.imported")));
        }
        Ok(store)
    }

    #[cfg(test)]
    fn in_memory() -> Store {
        let mut store = Store {
            conn: Connection::open_in_memory().unwrap(),
        };
        store
            .conn
            .execute_batch("PRAGMA foreign_keys = ON;")
            .unwrap();
        store.migrate().unwrap();
        store
    }

    fn migrate(&mut self) -> Result<(), String> {
        let version: usize = self
            .conn
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .map_err(err)?;
        if version > MIGRATIONS.len() {
            return Err("This project was made by a newer version of Sweeps Tracker.".into());
        }
        for (i, sql) in MIGRATIONS.iter().enumerate().skip(version) {
            let tx = self.conn.transaction().map_err(err)?;
            tx.execute_batch(sql).map_err(err)?;
            tx.pragma_update(None, "user_version", i + 1).map_err(err)?;
            tx.commit().map_err(err)?;
        }
        Ok(())
    }

    /// Everything that hasn't been deleted, casinos sorted by name.
    pub fn load(&self) -> Result<Data, String> {
        let mut casinos: Vec<Casino> = self
            .conn
            .prepare("SELECT id, name FROM casinos WHERE deleted_at IS NULL ORDER BY name COLLATE NOCASE")
            .map_err(err)?
            .query_map([], |r| {
                Ok(Casino {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    purchases: Vec::new(),
                    redemptions: Vec::new(),
                    free_coins: Vec::new(),
                })
            })
            .map_err(err)?
            .collect::<Result<_, _>>()
            .map_err(err)?;
        let index = |casinos: &[Casino], id: i64| casinos.iter().position(|c| c.id == id);

        let mut stmt = self
            .conn
            .prepare(
                "SELECT casino_id, id, date, spent_cents, cc_received_cents, note FROM purchases
                 WHERE deleted_at IS NULL ORDER BY date, id",
            )
            .map_err(err)?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    Purchase {
                        id: r.get(1)?,
                        date: r.get(2)?,
                        spent: r.get(3)?,
                        cc_received: r.get(4)?,
                        note: r.get(5)?,
                    },
                ))
            })
            .map_err(err)?;
        for row in rows {
            let (casino_id, p) = row.map_err(err)?;
            if let Some(i) = index(&casinos, casino_id) {
                casinos[i].purchases.push(p);
            }
        }

        let mut stmt = self
            .conn
            .prepare(
                "SELECT casino_id, id, date, amount_cents, method, fee_cents, received_date, note
                 FROM redemptions WHERE deleted_at IS NULL ORDER BY date, id",
            )
            .map_err(err)?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    Redemption {
                        id: r.get(1)?,
                        date: r.get(2)?,
                        amount: r.get(3)?,
                        method: Method::from_key(&r.get::<_, String>(4)?),
                        fee: r.get(5)?,
                        received: r.get(6)?,
                        note: r.get(7)?,
                    },
                ))
            })
            .map_err(err)?;
        for row in rows {
            let (casino_id, r) = row.map_err(err)?;
            if let Some(i) = index(&casinos, casino_id) {
                casinos[i].redemptions.push(r);
            }
        }

        let mut stmt = self
            .conn
            .prepare(
                "SELECT casino_id, id, date, sc_cents, source, note FROM free_coins
                 WHERE deleted_at IS NULL ORDER BY date, id",
            )
            .map_err(err)?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    FreeCoins {
                        id: r.get(1)?,
                        date: r.get(2)?,
                        sc: r.get(3)?,
                        source: Source::from_key(&r.get::<_, String>(4)?),
                        note: r.get(5)?,
                    },
                ))
            })
            .map_err(err)?;
        for row in rows {
            let (casino_id, f) = row.map_err(err)?;
            if let Some(i) = index(&casinos, casino_id) {
                casinos[i].free_coins.push(f);
            }
        }
        Ok(Data { casinos })
    }

    // ---- Casinos ----

    pub fn add_casino(&self, name: &str) -> Result<i64, String> {
        self.conn
            .execute("INSERT INTO casinos (name) VALUES (?1)", [name])
            .map_err(err)?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn rename_casino(&self, id: i64, name: &str) -> Result<(), String> {
        self.conn
            .execute(
                &format!("UPDATE casinos SET name = ?2, updated_at = {NOW} WHERE id = ?1"),
                params![id, name],
            )
            .map(|_| ())
            .map_err(err)
    }

    /// Soft-deletes the casino and its entries, marking those entries as deleted with the
    /// casino so `restore_casino` brings back exactly them (entries deleted earlier on
    /// their own stay deleted).
    pub fn delete_casino(&mut self, id: i64) -> Result<(), String> {
        let tx = self.conn.transaction().map_err(err)?;
        for table in ENTRY_TABLES {
            tx.execute(
                &format!(
                    "UPDATE {table} SET deleted_at = {NOW}, deleted_with_casino = 1
                     WHERE casino_id = ?1 AND deleted_at IS NULL"
                ),
                [id],
            )
            .map_err(err)?;
        }
        tx.execute(
            &format!("UPDATE casinos SET deleted_at = {NOW} WHERE id = ?1"),
            [id],
        )
        .map_err(err)?;
        tx.commit().map_err(err)
    }

    /// Un-deletes a casino and the entries that were deleted along with it. Fails if
    /// another casino now has the same name.
    pub fn restore_casino(&mut self, id: i64) -> Result<(), String> {
        let tx = self.conn.transaction().map_err(err)?;
        tx.execute(
            &format!("UPDATE casinos SET deleted_at = NULL, updated_at = {NOW} WHERE id = ?1"),
            [id],
        )
        .map_err(err)?;
        for table in ENTRY_TABLES {
            tx.execute(
                &format!(
                    "UPDATE {table} SET deleted_at = NULL, deleted_with_casino = 0
                     WHERE casino_id = ?1 AND deleted_with_casino = 1"
                ),
                [id],
            )
            .map_err(err)?;
        }
        tx.commit().map_err(err)
    }

    // ---- Entries ----

    pub fn delete_entry(&self, entry: EntryRef) -> Result<(), String> {
        self.conn
            .execute(
                &format!(
                    "UPDATE {} SET deleted_at = {NOW}, deleted_with_casino = 0 WHERE id = ?1",
                    entry.table()
                ),
                [entry.id()],
            )
            .map(|_| ())
            .map_err(err)
    }

    pub fn restore_entry(&self, entry: EntryRef) -> Result<(), String> {
        self.conn
            .execute(
                &format!(
                    "UPDATE {} SET deleted_at = NULL WHERE id = ?1",
                    entry.table()
                ),
                [entry.id()],
            )
            .map(|_| ())
            .map_err(err)
    }

    pub fn add_purchase(
        &self,
        casino_id: i64,
        date: NaiveDate,
        spent: Cents,
        cc_received: Cents,
        note: &str,
    ) -> Result<i64, String> {
        self.conn
            .execute(
                "INSERT INTO purchases (casino_id, date, spent_cents, cc_received_cents, note)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![casino_id, date, spent, cc_received, note],
            )
            .map_err(err)?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn update_purchase(
        &self,
        id: i64,
        date: NaiveDate,
        spent: Cents,
        cc_received: Cents,
        note: &str,
    ) -> Result<(), String> {
        self.conn
            .execute(
                &format!(
                    "UPDATE purchases SET date = ?2, spent_cents = ?3, cc_received_cents = ?4,
                     note = ?5, updated_at = {NOW} WHERE id = ?1"
                ),
                params![id, date, spent, cc_received, note],
            )
            .map(|_| ())
            .map_err(err)
    }

    pub fn add_redemption(&self, casino_id: i64, r: &RedemptionInput) -> Result<i64, String> {
        insert_redemption(&self.conn, casino_id, r)?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn update_redemption(&self, id: i64, r: &RedemptionInput) -> Result<(), String> {
        self.conn
            .execute(
                &format!(
                    "UPDATE redemptions SET date = ?2, amount_cents = ?3, method = ?4,
                     fee_cents = ?5, received_date = ?6, note = ?7, updated_at = {NOW}
                     WHERE id = ?1"
                ),
                params![
                    id,
                    r.date,
                    r.amount,
                    r.method.key(),
                    r.fee,
                    r.received,
                    r.note
                ],
            )
            .map(|_| ())
            .map_err(err)
    }

    /// Marks a pending redemption as received on `date`.
    pub fn mark_received(&self, id: i64, date: NaiveDate) -> Result<(), String> {
        self.conn
            .execute(
                &format!(
                    "UPDATE redemptions SET received_date = ?2, updated_at = {NOW} WHERE id = ?1"
                ),
                params![id, date],
            )
            .map(|_| ())
            .map_err(err)
    }

    pub fn add_free_coins(
        &self,
        casino_id: i64,
        date: NaiveDate,
        sc: Cents,
        source: Source,
        note: &str,
    ) -> Result<i64, String> {
        self.conn
            .execute(
                "INSERT INTO free_coins (casino_id, date, sc_cents, source, note)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![casino_id, date, sc, source.key(), note],
            )
            .map_err(err)?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn update_free_coins(
        &self,
        id: i64,
        date: NaiveDate,
        sc: Cents,
        source: Source,
        note: &str,
    ) -> Result<(), String> {
        self.conn
            .execute(
                &format!(
                    "UPDATE free_coins SET date = ?2, sc_cents = ?3, source = ?4, note = ?5,
                     updated_at = {NOW} WHERE id = ?1"
                ),
                params![id, date, sc, source.key(), note],
            )
            .map(|_| ())
            .map_err(err)
    }

    /// Deleted casinos, and entries deleted on their own from casinos that still exist,
    /// newest first.
    pub fn deleted(&self) -> Result<Deleted, String> {
        let casinos = self
            .conn
            .prepare(
                "SELECT c.id, c.name, c.deleted_at,
                        (SELECT count(*) FROM purchases p WHERE p.casino_id = c.id AND p.deleted_with_casino = 1)
                      + (SELECT count(*) FROM redemptions r WHERE r.casino_id = c.id AND r.deleted_with_casino = 1)
                      + (SELECT count(*) FROM free_coins f WHERE f.casino_id = c.id AND f.deleted_with_casino = 1)
                 FROM casinos c WHERE c.deleted_at IS NOT NULL ORDER BY c.deleted_at DESC",
            )
            .map_err(err)?
            .query_map([], |r| {
                Ok(DeletedCasino {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    deleted_at: r.get(2)?,
                    entries: r.get(3)?,
                })
            })
            .map_err(err)?
            .collect::<Result<_, _>>()
            .map_err(err)?;

        let mut entries: Vec<DeletedEntry> = Vec::new();
        let queries: [(&str, &str, fn(i64) -> EntryRef); 3] = [
            ("purchases", "spent_cents", EntryRef::Purchase),
            ("redemptions", "amount_cents", EntryRef::Redemption),
            ("free_coins", "sc_cents", EntryRef::FreeCoins),
        ];
        for (table, amount, kind) in queries {
            let mut stmt = self
                .conn
                .prepare(&format!(
                    "SELECT e.id, c.name, e.date, e.{amount}, e.deleted_at FROM {table} e
                     JOIN casinos c ON c.id = e.casino_id
                     WHERE e.deleted_at IS NOT NULL AND c.deleted_at IS NULL"
                ))
                .map_err(err)?;
            let rows = stmt
                .query_map([], |r| {
                    Ok(DeletedEntry {
                        entry: kind(r.get(0)?),
                        casino: r.get(1)?,
                        date: r.get(2)?,
                        amount: r.get(3)?,
                        deleted_at: r.get(4)?,
                    })
                })
                .map_err(err)?;
            for row in rows {
                entries.push(row.map_err(err)?);
            }
        }
        entries.sort_by(|a, b| b.deleted_at.cmp(&a.deleted_at));
        Ok(Deleted { casinos, entries })
    }

    // ---- Import / backup ----

    /// Adds pasted spreadsheet rows in one transaction: a row's spent/CC becomes a purchase
    /// and its withdrawal a (received, bank) redemption.
    pub fn import_rows(&mut self, casino_id: i64, rows: &[ParsedRow]) -> Result<(), String> {
        let tx = self.conn.transaction().map_err(err)?;
        for r in rows {
            insert_parts(
                &tx,
                casino_id,
                r.date,
                r.spent,
                r.cc_received,
                r.withdrawal,
                "",
            )?;
        }
        tx.commit().map_err(err)
    }

    /// Imports a project saved in the old sweeps.json format (float dollars, one combined
    /// row type) in one transaction.
    fn import_legacy_json(&mut self, path: &Path) -> Result<(), String> {
        let text = fs::read_to_string(path).map_err(|e| e.to_string())?;
        let legacy: LegacyData = serde_json::from_str(&text)
            .map_err(|e| format!("Couldn't read {}: {e}", path.display()))?;
        let tx = self.conn.transaction().map_err(err)?;
        for c in &legacy.casinos {
            tx.execute("INSERT INTO casinos (name) VALUES (?1)", [&c.name])
                .map_err(err)?;
            let casino_id = tx.last_insert_rowid();
            for e in &c.entries {
                insert_parts(
                    &tx,
                    casino_id,
                    e.date,
                    legacy_cents(e.spent),
                    legacy_cents(e.cc_received),
                    legacy_cents(e.withdrawal),
                    &e.note,
                )?;
            }
        }
        tx.commit().map_err(err)
    }

    /// Writes a consistent copy of the database to `target` (which must not exist yet).
    pub fn copy_to(&self, target: &Path) -> Result<(), String> {
        self.conn
            .execute("VACUUM INTO ?1", [target.to_string_lossy()])
            .map(|_| ())
            .map_err(err)
    }

    /// Saves backups/sweeps-YYYY-MM-DD.db (once per day) and keeps the newest
    /// `BACKUPS_KEPT` backups.
    pub fn backup(&self, dir: &Path) -> Result<(), String> {
        let backups = dir.join(BACKUP_DIR);
        fs::create_dir_all(&backups).map_err(|e| e.to_string())?;
        let today = chrono::Local::now().format("%Y-%m-%d");
        let target = backups.join(format!("sweeps-{today}.db"));
        if !target.exists() {
            self.copy_to(&target)?;
        }
        prune_backups(&backups)
    }
}

fn insert_redemption(conn: &Connection, casino_id: i64, r: &RedemptionInput) -> Result<(), String> {
    conn.execute(
        "INSERT INTO redemptions (casino_id, date, amount_cents, method, fee_cents, received_date, note)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![casino_id, r.date, r.amount, r.method.key(), r.fee, r.received, r.note],
    )
    .map(|_| ())
    .map_err(err)
}

/// Inserts the purchase and/or redemption described by one combined (spreadsheet) row.
/// Imported redemptions are assumed received by bank on their date.
fn insert_parts(
    conn: &Connection,
    casino_id: i64,
    date: NaiveDate,
    spent: Cents,
    cc_received: Cents,
    withdrawal: Cents,
    note: &str,
) -> Result<(), String> {
    if spent > 0 || cc_received > 0 {
        conn.execute(
            "INSERT INTO purchases (casino_id, date, spent_cents, cc_received_cents, note)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![casino_id, date, spent.max(0), cc_received.max(0), note],
        )
        .map_err(err)?;
    }
    if withdrawal > 0 {
        let r = RedemptionInput {
            date,
            amount: withdrawal,
            method: Method::Bank,
            fee: 0,
            received: Some(date),
            note: note.to_string(),
        };
        insert_redemption(conn, casino_id, &r)?;
    }
    Ok(())
}

/// Backup names sort by date, so the oldest come first.
fn prune_backups(backups: &Path) -> Result<(), String> {
    let mut existing: Vec<PathBuf> = fs::read_dir(backups)
        .map_err(|e| e.to_string())?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with("sweeps-"))
        })
        .collect();
    existing.sort();
    while existing.len() > BACKUPS_KEPT {
        let _ = fs::remove_file(existing.remove(0));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 3, d).unwrap()
    }

    fn redemption(date: NaiveDate, amount: Cents) -> RedemptionInput {
        RedemptionInput {
            date,
            amount,
            method: Method::Bank,
            fee: 0,
            received: Some(date),
            note: String::new(),
        }
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sweeps-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn adds_edits_and_soft_deletes() {
        let mut s = Store::in_memory();
        let modo = s.add_casino("Modo").unwrap();
        let p = s.add_purchase(modo, day(13), 8500, 10000, "first").unwrap();
        s.add_redemption(modo, &redemption(day(14), 50000)).unwrap();
        s.update_purchase(p, day(13), 9000, 10000, "fixed").unwrap();

        let data = s.load().unwrap();
        let c = &data.casinos[0];
        assert_eq!(c.purchases[0].spent, 9000);
        assert_eq!(c.purchases[0].note, "fixed");
        assert_eq!(c.stats().net(), 41000);

        s.delete_entry(EntryRef::Purchase(p)).unwrap();
        assert!(s.load().unwrap().casinos[0].purchases.is_empty());
        // Soft delete: the row is still in the table.
        let kept: i64 = s
            .conn
            .query_row(
                "SELECT count(*) FROM purchases WHERE deleted_at IS NOT NULL",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(kept, 1);

        s.delete_casino(modo).unwrap();
        assert!(s.load().unwrap().casinos.is_empty());
        // A deleted casino's name can be reused.
        s.add_casino("Modo").unwrap();
    }

    #[test]
    fn stores_redemption_tax_fields_and_free_coins() {
        let s = Store::in_memory();
        let id = s.add_casino("McLuck").unwrap();
        let pending = RedemptionInput {
            method: Method::GiftCard,
            fee: 150,
            received: None,
            note: "amazon".into(),
            ..redemption(day(20), 10000)
        };
        let r = s.add_redemption(id, &pending).unwrap();
        s.add_free_coins(id, day(21), 250, Source::MailIn, "postcard")
            .unwrap();

        let c = &s.load().unwrap().casinos[0];
        let got = &c.redemptions[0];
        assert_eq!(
            (got.method, got.fee, got.received),
            (Method::GiftCard, 150, None)
        );
        assert!(got.is_pending());
        assert_eq!(c.free_coins[0].source, Source::MailIn);
        assert_eq!(c.free_coins[0].sc, 250);

        s.mark_received(r, day(25)).unwrap();
        assert_eq!(
            s.load().unwrap().casinos[0].redemptions[0].received,
            Some(day(25))
        );
    }

    #[test]
    fn upgrades_a_version_1_database() {
        let mut s = Store {
            conn: Connection::open_in_memory().unwrap(),
        };
        // Build a version-1 database with a casino deleted along with one of its entries.
        s.conn.execute_batch(MIGRATIONS[0]).unwrap();
        s.conn.pragma_update(None, "user_version", 1).unwrap();
        s.conn
            .execute_batch(
                "INSERT INTO casinos (id, name) VALUES (1, 'Zoot');
                 INSERT INTO redemptions (casino_id, date, amount_cents) VALUES (1, '2026-03-10', 5000);
                 INSERT INTO casinos (id, name, deleted_at) VALUES (2, 'Old', '2026-03-01T00:00:00.000Z');
                 INSERT INTO purchases (casino_id, date, spent_cents, cc_received_cents, deleted_at)
                     VALUES (2, '2026-02-01', 100, 100, '2026-03-01T00:00:00.000Z');",
            )
            .unwrap();
        s.migrate().unwrap();

        let r = &s.load().unwrap().casinos[0].redemptions[0];
        assert_eq!(
            (r.method, r.fee, r.received),
            (Method::Bank, 0, Some(day(10)))
        );
        // The old casino's entry is recognized as deleted with it, so restoring brings it back.
        s.restore_casino(2).unwrap();
        let old = s
            .load()
            .unwrap()
            .casinos
            .into_iter()
            .find(|c| c.id == 2)
            .unwrap();
        assert_eq!(old.purchases.len(), 1);
    }

    #[test]
    fn restores_casinos_and_entries() {
        let mut s = Store::in_memory();
        let modo = s.add_casino("Modo").unwrap();
        let early = s.add_purchase(modo, day(1), 1000, 1000, "").unwrap();
        s.add_purchase(modo, day(2), 2000, 2000, "").unwrap();
        s.add_redemption(modo, &redemption(day(3), 5000)).unwrap();
        s.add_free_coins(modo, day(3), 100, Source::Daily, "")
            .unwrap();
        s.delete_entry(EntryRef::Purchase(early)).unwrap(); // deleted on its own first

        let listed = s.deleted().unwrap();
        assert_eq!(listed.entries.len(), 1);
        assert_eq!(listed.entries[0].entry, EntryRef::Purchase(early));
        assert_eq!(listed.entries[0].casino, "Modo");

        s.delete_casino(modo).unwrap();
        let listed = s.deleted().unwrap();
        assert_eq!(listed.casinos.len(), 1);
        assert_eq!(listed.casinos[0].entries, 3);
        // Entries of a deleted casino are listed under the casino, not on their own.
        assert!(listed.entries.is_empty());

        s.restore_casino(modo).unwrap();
        let c = &s.load().unwrap().casinos[0];
        assert_eq!(
            (c.purchases.len(), c.redemptions.len(), c.free_coins.len()),
            (1, 1, 1)
        );
        // The purchase deleted on its own earlier is still deleted, and restorable.
        s.restore_entry(EntryRef::Purchase(early)).unwrap();
        assert_eq!(s.load().unwrap().casinos[0].purchases.len(), 2);

        // Restoring fails cleanly if the name has been reused meanwhile.
        s.delete_casino(modo).unwrap();
        s.add_casino("modo").unwrap();
        assert_eq!(
            s.restore_casino(modo).unwrap_err(),
            "You already have a casino with that name."
        );
    }

    #[test]
    fn rejects_duplicate_names_and_bad_amounts() {
        let s = Store::in_memory();
        let id = s.add_casino("Zoot").unwrap();
        assert_eq!(
            s.add_casino("zoot").unwrap_err(),
            "You already have a casino with that name."
        );
        assert!(s.add_redemption(id, &redemption(day(1), 0)).is_err());
        assert!(s.add_purchase(id, day(1), -5, 100, "").is_err());
        assert!(s.add_free_coins(id, day(1), 0, Source::Daily, "").is_err());
        let negative_fee = RedemptionInput {
            fee: -1,
            ..redemption(day(1), 100)
        };
        assert!(s.add_redemption(id, &negative_fee).is_err());
    }

    #[test]
    fn imports_rows_and_splits_them() {
        let mut s = Store::in_memory();
        let id = s.add_casino("Spree").unwrap();
        let rows = crate::model::parse_spreadsheet(
            "3/13/2026\t90\t100\t\n3/14/2026\t\t\t500\n3/15/2026\t18\t20\t40",
        );
        s.import_rows(id, &rows).unwrap();
        let c = &s.load().unwrap().casinos[0];
        assert_eq!(c.purchases.len(), 2);
        assert_eq!(c.redemptions.len(), 2);
        assert_eq!(c.stats().spent, 10800);
        assert_eq!(c.stats().withdrawn, 54000);
        assert_eq!(c.redemptions[0].received, Some(day(14)));
        assert_eq!(c.redemptions[0].method, Method::Bank);
    }

    #[test]
    fn imports_legacy_json_project_once() {
        let dir = temp_dir("legacy");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join(LEGACY_PROJECT_FILE),
            r#"{"next_id": 3, "casinos": [{"id": 1, "name": "McLuck", "entries": [
                {"id": 2, "date": "2026-03-16", "spent": 37.5, "cc_received": 50.0, "withdrawal": 0.0},
                {"id": 3, "date": "2026-03-18", "spent": 0.0, "cc_received": 0.0, "withdrawal": 577.0, "note": "paid"}
            ]}]}"#,
        )
        .unwrap();
        {
            let s = Store::open(&dir).unwrap();
            let c = &s.load().unwrap().casinos[0];
            assert_eq!(c.name, "McLuck");
            assert_eq!(c.purchases[0].spent, 3750);
            assert_eq!(c.redemptions[0].amount, 57700);
            assert_eq!(c.redemptions[0].note, "paid");
        }
        assert!(!dir.join(LEGACY_PROJECT_FILE).exists());
        assert!(dir.join("sweeps.json.imported").exists());
        // Reopening doesn't import again.
        let s = Store::open(&dir).unwrap();
        assert_eq!(s.load().unwrap().casinos.len(), 1);
        drop(s);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn second_open_of_the_same_project_is_refused() {
        let dir = temp_dir("lock");
        let first = Store::open(&dir).unwrap();
        let second = Store::open(&dir);
        assert_eq!(
            second.err().unwrap(),
            "This project is already open in another Sweeps Tracker window."
        );
        drop(first);
        assert!(Store::open(&dir).is_ok());
        fs::remove_dir_all(&dir).unwrap();
    }

    /// Dev check against a real project folder (use a copy):
    /// PROJECT_DIR=path cargo test opens_project_from_env -- --ignored --nocapture
    #[test]
    #[ignore]
    fn opens_project_from_env() {
        let dir = PathBuf::from(std::env::var("PROJECT_DIR").expect("set PROJECT_DIR"));
        let s = Store::open(&dir).unwrap();
        let version: i64 = s
            .conn
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        let data = s.load().unwrap();
        println!("schema version {version}");
        for c in &data.casinos {
            println!(
                "{}: {} purchases, {} redemptions, {} free SC",
                c.name,
                c.purchases.len(),
                c.redemptions.len(),
                c.free_coins.len()
            );
        }
    }

    #[test]
    fn backs_up_once_a_day_and_keeps_the_newest() {
        let dir = temp_dir("backup");
        let backups = dir.join(BACKUP_DIR);
        fs::create_dir_all(&backups).unwrap();
        for d in 1..=BACKUPS_KEPT + 5 {
            fs::write(backups.join(format!("sweeps-2000-01-{d:02}.db")), "").unwrap();
        }
        let s = Store::open(&dir).unwrap();
        s.add_casino("Modo").unwrap();
        s.backup(&dir).unwrap();
        s.backup(&dir).unwrap(); // same day: nothing new
        let mut names: Vec<String> = fs::read_dir(&backups)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(names.len(), BACKUPS_KEPT);
        let today = chrono::Local::now()
            .format("sweeps-%Y-%m-%d.db")
            .to_string();
        assert_eq!(names.last().unwrap(), &today);
        // The backup is a real, readable database.
        let copy = Connection::open(backups.join(&today)).unwrap();
        let n: i64 = copy
            .query_row("SELECT count(*) FROM casinos", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1);
        drop(s);
        fs::remove_dir_all(&dir).unwrap();
    }
}
