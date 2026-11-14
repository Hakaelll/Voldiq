use super::{Action, HumanAuthority, Store, invalid, recorded_manifest};
use crate::{Error, Result};
use rusqlite::{OptionalExtension, Transaction, TransactionBehavior};
use serde::Serialize;

pub(in crate::plans) const SCHEMA: &str = "
CREATE TABLE recovery_vault_quota (
    id INTEGER PRIMARY KEY CHECK(id=1),
    limit_bytes INTEGER NOT NULL CHECK(limit_bytes>0)
);
PRAGMA user_version=5;";

pub(in crate::plans) const RETENTION_SCHEMA: &str = "
CREATE TABLE recovery_vault_retention (
    id INTEGER PRIMARY KEY CHECK(id=1),
    days INTEGER NOT NULL CHECK(days BETWEEN 1 AND 3650)
);
PRAGMA user_version=6;";

#[derive(Serialize)]
pub struct VaultQuota {
    pub limit_bytes: Option<String>,
    pub charged_bytes: String,
    pub charged_plans: usize,
    pub retention_days: Option<u16>,
}

pub(super) fn retention_days(tx: &Transaction<'_>) -> Result<Option<u16>> {
    Ok(tx
        .query_row(
            "SELECT days FROM recovery_vault_retention WHERE id=1",
            [],
            |row| row.get(0),
        )
        .optional()?)
}

pub(in crate::plans) fn review_at(
    manifest: &super::Manifest,
    file: &super::TransferFile,
    days: Option<u16>,
) -> Option<i64> {
    if matches!(manifest.action, Action::Offload(_)) {
        file.published_at?.checked_add(i64::from(days?) * 86_400)
    } else {
        None
    }
}

fn limit(tx: &Transaction<'_>) -> Result<Option<u64>> {
    let value: Option<i64> = tx
        .query_row(
            "SELECT limit_bytes FROM recovery_vault_quota WHERE id=1",
            [],
            |row| row.get(0),
        )
        .optional()?;
    value
        .map(|bytes| {
            u64::try_from(bytes)
                .ok()
                .filter(|bytes| *bytes > 0)
                .ok_or_else(invalid)
        })
        .transpose()
}

fn allowance(action: &Action) -> Result<u64> {
    let Action::Offload(offload) = action else {
        return Ok(0);
    };
    offload
        .required_available_bytes
        .parse::<u64>()
        .map_err(|_| invalid())?
        .checked_sub(super::super::offload::RESERVE)
        .ok_or_else(invalid)
}

fn usage(tx: &Transaction<'_>) -> Result<(u128, usize)> {
    // Keep the whole estimate once copying might have started. A ledger cannot prove
    // that an interrupted output was removed or that the old device is still connected.
    let mut statement = tx.prepare(
        "SELECT p.id FROM transfer_operations o JOIN agent_plans p ON p.id=o.plan_id
         WHERE p.state='executing'
           OR EXISTS(SELECT 1 FROM plan_audit a WHERE a.plan_id=p.id AND a.event='executing')
           OR EXISTS(SELECT 1 FROM transfer_files f WHERE f.plan_id=p.id AND f.private_name IS NOT NULL)
         LIMIT 10001",
    )?;
    let mut rows = statement.query([])?;
    let mut bytes = 0_u128;
    let mut plans = 0;
    let mut scanned = 0;
    while let Some(row) = rows.next()? {
        scanned += 1;
        if scanned > 10_000 {
            return Err(invalid());
        }
        let plan = recorded_manifest(tx, &row.get::<_, String>(0)?)?;
        let charge = allowance(&plan.manifest.action)?;
        if charge != 0 {
            bytes += u128::from(charge);
            plans += 1;
        }
    }
    Ok((bytes, plans))
}

pub(in crate::plans) fn admit(tx: &Transaction<'_>, action: &Action) -> Result<()> {
    if !matches!(action, Action::Offload(_)) {
        return Ok(());
    }
    let Some(limit) = limit(tx)? else {
        return Ok(());
    };
    let (charged, _) = usage(tx)?;
    if charged + u128::from(allowance(action)?) > u128::from(limit) {
        return Err(Error::Invalid(
            "The recovery vault quota would be exceeded. Review it in Cleanup → Transfers.".into(),
        ));
    }
    Ok(())
}

impl Store {
    pub(in crate::plans) fn vault_quota(&self, human: &HumanAuthority) -> Result<VaultQuota> {
        human.check_directory(self.directory.identity())?;
        let tx = self.connection.unchecked_transaction()?;
        let limit_bytes = limit(&tx)?.map(|value| value.to_string());
        let (bytes, charged_plans) = usage(&tx)?;
        human.check_directory(self.directory.identity())?;
        Ok(VaultQuota {
            limit_bytes,
            charged_bytes: bytes.to_string(),
            charged_plans,
            retention_days: retention_days(&tx)?,
        })
    }

    pub(in crate::plans) fn set_vault_quota(
        &mut self,
        human: &HumanAuthority,
        limit: Option<&str>,
    ) -> Result<()> {
        human.check_directory(self.directory.identity())?;
        let limit = limit
            .map(|text| {
                let error = || {
                    Error::Invalid(
                        "Enter a positive whole byte count up to 9,223,372,036,854,775,807.".into(),
                    )
                };
                if text.len() > 19 {
                    return Err(error());
                }
                let bytes = text.parse::<i64>().map_err(|_| error())?;
                if bytes <= 0 || bytes.to_string() != text {
                    return Err(error());
                }
                Ok(bytes)
            })
            .transpose()?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(bytes) = limit {
            tx.execute(
                "INSERT INTO recovery_vault_quota(id,limit_bytes) VALUES(1,?1)
                 ON CONFLICT(id) DO UPDATE SET limit_bytes=excluded.limit_bytes",
                [bytes],
            )?;
        } else {
            tx.execute("DELETE FROM recovery_vault_quota WHERE id=1", [])?;
        }
        human.check_directory(self.directory.identity())?;
        tx.commit()?;
        Ok(())
    }

    pub(in crate::plans) fn set_vault_retention(
        &mut self,
        human: &HumanAuthority,
        days: Option<u16>,
    ) -> Result<()> {
        human.check_directory(self.directory.identity())?;
        if days.is_some_and(|days| !(1..=3650).contains(&days)) {
            return Err(Error::Invalid(
                "Choose a retention period from 1 to 3650 days.".into(),
            ));
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(days) = days {
            tx.execute(
                "INSERT INTO recovery_vault_retention(id,days) VALUES(1,?1)
                 ON CONFLICT(id) DO UPDATE SET days=excluded.days",
                [days],
            )?;
        } else {
            tx.execute("DELETE FROM recovery_vault_retention WHERE id=1", [])?;
        }
        human.check_directory(self.directory.identity())?;
        tx.commit()?;
        Ok(())
    }
}
