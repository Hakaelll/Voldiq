//! Durable intents only. A request never replaces the desktop's live single-use grant.
use super::{Store, audit};
use crate::{
    Error, Result,
    agents::{Capability, PlanContext},
    plans::{HumanAuthority, State, invalid},
};
use rusqlite::{OptionalExtension, TransactionBehavior, params};
use serde::Serialize;

pub enum Enqueue {
    Queued,
    Existing(State),
}

#[derive(Debug, Serialize)]
pub struct ExecutionRequest {
    pub plan_id: String,
    pub manifest_hash: String,
    pub connection_id: i64,
    pub requested: i64,
    pub cancel_requested: bool,
}

impl Store {
    pub fn request_execution(
        &mut self,
        context: &PlanContext<'_>,
        id: &str,
        expected_hash: &str,
        now: i64,
    ) -> Result<Enqueue> {
        context.check(Capability::ExecuteApproved)?;
        let stored = self.load(context, id, now)?;
        let view = &stored.view;
        if view.hash != expected_hash {
            return Err(invalid());
        }
        if view.state.terminal() || view.state == State::Executing {
            return Ok(Enqueue::Existing(view.state));
        }
        if view.state != State::Approved {
            return Err(Error::NeedsHumanApproval);
        }
        self.validate_material(context, &stored, now)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let issuer: String = tx.query_row(
            "SELECT approval_issuer FROM agent_plans WHERE id=?1 AND owner=?2 AND hash=?3 AND state='approved' AND approval_expires>?4",
            params![id, context.owner(), expected_hash, now], |row| row.get(0),
        ).optional()?.ok_or(Error::NeedsHumanApproval)?;
        if issuer.len() != 64 || !issuer.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(invalid());
        }
        let previous: Option<(String, i64, String)> = tx
            .query_row(
                "SELECT issuer,owner,hash FROM execution_requests WHERE plan_id=?1",
                [id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        if let Some((previous_issuer, owner, hash)) = &previous {
            if *owner != context.owner() || hash != expected_hash {
                return Err(invalid());
            }
            if previous_issuer == &issuer {
                return Ok(Enqueue::Queued);
            }
        } else {
            let count: i64 =
                tx.query_row("SELECT count(*) FROM execution_requests", [], |row| {
                    row.get(0)
                })?;
            if count >= 10_000 {
                return Err(invalid());
            }
        }
        tx.execute("INSERT INTO execution_requests(plan_id,owner,hash,issuer,requested) VALUES(?1,?2,?3,?4,?5)
            ON CONFLICT(plan_id) DO UPDATE SET issuer=excluded.issuer,requested=excluded.requested,cancel_requested=0",
            params![id,context.owner(),expected_hash,issuer,now])?;
        audit(&tx, id, now, "execution_requested")?;
        tx.commit()?;
        Ok(Enqueue::Queued)
    }

    /// Bounded discovery for the current human session. Dispatch must still consume its live grant.
    pub fn pending_execution(
        &self,
        human: &HumanAuthority,
        limit: usize,
        now: i64,
    ) -> Result<Vec<ExecutionRequest>> {
        human.check_directory(self.directory.identity())?;
        if !(1..=100).contains(&limit) || now < 0 {
            return Err(invalid());
        }
        let mut statement = self.connection.prepare("SELECT r.plan_id,r.hash,r.owner,r.requested,r.cancel_requested
            FROM execution_requests r JOIN agent_plans p ON p.id=r.plan_id AND p.owner=r.owner AND p.hash=r.hash
            WHERE r.issuer=?1 AND p.approval_issuer=r.issuer AND p.state='approved'
                AND p.approval_expires>?2 AND r.cancel_requested=0 AND r.requested<=?2
                AND r.requested>=0 AND length(r.plan_id)=64 AND length(r.hash)=64 AND length(r.issuer)=64
            ORDER BY r.requested,r.plan_id LIMIT ?3")?;
        Ok(statement
            .query_map(params![human.issuer(), now, limit as i64], request_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?)
    }

    pub fn execution_request(
        &self,
        context: &PlanContext<'_>,
        id: &str,
        expected_hash: &str,
    ) -> Result<Option<ExecutionRequest>> {
        context.check(Capability::ExecuteApproved)?;
        let stored = self.read(context, id)?;
        if stored.view.hash != expected_hash {
            return Err(invalid());
        }
        Ok(self.connection.query_row(
            "SELECT r.plan_id,r.hash,r.owner,r.requested,r.cancel_requested FROM execution_requests r
                JOIN agent_plans p ON p.id=r.plan_id AND p.approval_issuer=r.issuer
                WHERE r.plan_id=?1 AND r.owner=?2 AND r.hash=?3 AND r.requested>=0",
            params![id,context.owner(),expected_hash],request_row,
        ).optional()?)
    }

    /// Before execution this cancels the plan. During execution it asks the worker to stop safely.
    pub fn cancel_execution(
        &mut self,
        context: &PlanContext<'_>,
        id: &str,
        expected_hash: &str,
        now: i64,
    ) -> Result<State> {
        context.check(Capability::ExecuteApproved)?;
        let stored = self.load(context, id, now)?;
        if stored.view.hash != expected_hash {
            return Err(invalid());
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current: String = tx.query_row(
            "SELECT state FROM agent_plans WHERE id=?1 AND owner=?2 AND hash=?3",
            params![id, context.owner(), expected_hash],
            |row| row.get(0),
        )?;
        let current: State =
            serde_json::from_value(serde_json::Value::String(current)).map_err(|_| invalid())?;
        if current.terminal() {
            return Ok(current);
        }
        let changed = tx.execute("UPDATE execution_requests SET cancel_requested=1 WHERE plan_id=?1 AND owner=?2 AND hash=?3 AND cancel_requested=0",
            params![id,context.owner(),expected_hash])?;
        if current == State::Executing {
            let exists: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM execution_requests WHERE plan_id=?1 AND owner=?2 AND hash=?3)",
                params![id,context.owner(),expected_hash],|row| row.get(0))?;
            if !exists {
                return Err(invalid());
            }
            if changed == 1 {
                audit(&tx, id, now, "execution_cancel_requested")?;
            }
            tx.commit()?;
            return Ok(State::Executing);
        }
        tx.execute("UPDATE agent_plans SET state='cancelled',approval_expires=NULL,approval_issuer=NULL WHERE id=?1 AND owner=?2 AND hash=?3",
            params![id,context.owner(),expected_hash])?;
        audit(&tx, id, now, "cancelled")?;
        tx.commit()?;
        Ok(State::Cancelled)
    }
}

fn request_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ExecutionRequest> {
    Ok(ExecutionRequest {
        plan_id: row.get(0)?,
        manifest_hash: row.get(1)?,
        connection_id: row.get(2)?,
        requested: row.get(3)?,
        cancel_requested: row.get(4)?,
    })
}
