//! Form history from `formhistory.sqlite` (used once general autofill ships).

use rusqlite::{Connection, params};

use crate::error::Result;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormEntry {
    pub field_name: String,
    pub value: String,
    pub times_used: i64,
    pub first_used_us: Option<i64>,
    pub last_used_us: Option<i64>,
}

pub fn read(conn: &Connection) -> Result<Vec<FormEntry>> {
    let mut stmt =
        conn.prepare("SELECT fieldname, value, COALESCE(timesUsed, 1), firstUsed, lastUsed FROM moz_formhistory")?;
    let rows = stmt
        .query_map([], |r| {
            Ok(FormEntry {
                field_name: r.get(0)?,
                value: r.get(1)?,
                times_used: r.get(2)?,
                first_used_us: r.get(3)?,
                last_used_us: r.get(4)?,
            })
        })?
        .collect::<Result<_, _>>()?;
    Ok(rows)
}

/// Merges into Limbo's `form_history` (idempotent by field name + value).
pub fn store(conn: &mut Connection, entries: &[FormEntry]) -> Result<usize> {
    let tx = conn.transaction()?;
    let mut n = 0;
    {
        let mut stmt = tx.prepare(
            "INSERT INTO form_history(field_name, value, times_used, first_used_us, last_used_us)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(field_name, value) DO UPDATE SET
               times_used = MAX(times_used, excluded.times_used),
               last_used_us = MAX(COALESCE(last_used_us, 0), COALESCE(excluded.last_used_us, 0))",
        )?;
        for e in entries {
            if e.field_name.is_empty() || e.value.is_empty() {
                continue;
            }
            n += stmt.execute(params![e.field_name, e.value, e.times_used, e.first_used_us, e.last_used_us])?;
        }
    }
    tx.commit()?;
    Ok(n)
}
