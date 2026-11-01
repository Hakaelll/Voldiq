use super::*;
use crate::{
    NodeId,
    agents::{Capability, PlanContext, denied, pin_directory},
    cleanup::record::Record,
    index::Index,
};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use std::path::Path;

// Leave room for the result summary and envelope inside the 512 KiB transport limit.
const MANIFEST_LIMIT: usize = 440 * 1024;
const RECORD_LIMIT: usize = 2 * 1024 * 1024;

pub struct Store {
    connection: Connection,
    pub(super) directory: crate::agents::AuthorizedPath,
}

struct Stored {
    view: View,
    record: Record,
    binding: String,
}

impl Store {
    pub(super) fn review_refs(
        &self,
        human: &HumanAuthority,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<(String, i64)>> {
        human.check_directory(self.directory.identity())?;
        if offset > 10_000 || !(1..=100).contains(&limit) {
            return Err(invalid());
        }
        let mut statement = self.connection.prepare("SELECT id,owner FROM agent_plans WHERE state!='draft' ORDER BY rowid DESC LIMIT ?1 OFFSET ?2")?;
        Ok(statement
            .query_map(params![(limit + 1) as i64, offset as i64], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?)
    }

    pub(super) fn review_owner(&self, human: &HumanAuthority, id: &str) -> Result<i64> {
        human.check_directory(self.directory.identity())?;
        if id.len() != 64 || !id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(invalid());
        }
        self.connection
            .query_row("SELECT owner FROM agent_plans WHERE id=?1", [id], |row| {
                row.get(0)
            })
            .optional()?
            .ok_or_else(invalid)
    }

    pub fn open(directory: &Path) -> Result<Self> {
        let directory = pin_directory(directory)?;
        let mut connection = Connection::open(directory.path().join("agent-plans.db"))?;
        crate::database::configure(&connection)?;
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let version: u32 = tx.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        if version == 0 {
            tx.execute_batch("CREATE TABLE agent_plans(id TEXT PRIMARY KEY,owner INTEGER NOT NULL,manifest TEXT NOT NULL,
                hash TEXT NOT NULL,native_record TEXT NOT NULL,binding TEXT NOT NULL,state TEXT NOT NULL,
                approval_issuer TEXT,approval_expires INTEGER,execution_nonce TEXT,result TEXT);
                CREATE INDEX agent_plans_owner ON agent_plans(owner,id);
                CREATE TABLE plan_audit(sequence INTEGER PRIMARY KEY AUTOINCREMENT,plan_id TEXT NOT NULL REFERENCES agent_plans(id),at INTEGER NOT NULL,event TEXT NOT NULL);
                CREATE INDEX plan_audit_id ON plan_audit(plan_id,sequence);
                PRAGMA user_version=1;")?;
        } else if version != 1 {
            return Err(invalid());
        }
        tx.commit()?;
        Ok(Self {
            connection,
            directory,
        })
    }

    pub fn create(
        &mut self,
        context: &PlanContext<'_>,
        snapshot: i64,
        index: &Index,
        selection: &[NodeId],
        now: i64,
    ) -> Result<View> {
        let policy = context.check(Capability::PreparePlans)?;
        context.check_directory(self.directory.identity())?;
        if snapshot <= 0
            || index.snapshot_id.is_some_and(|id| id != snapshot)
            || selection.is_empty()
            || selection.len() > policy.limits().plan_files
            || now < 0
        {
            return Err(invalid());
        }
        let record = Record::capture(index, selection)?;
        let _root = context.snapshot(&record.scope())?;
        record.validate(self.directory.identity())?;
        let protections = crate::metadata::Store::open(self.directory.path())?.protections()?;
        record.check_protections(&protections)?;
        let entries = record.evidence()?;
        let _pins = entries
            .iter()
            .map(|entry| context.path(Path::new(&entry.path)))
            .collect::<Result<Vec<_>>>()?;
        let (logical, reclaim) = totals(&entries)?;
        if logical > policy.limits().plan_bytes {
            return Err(invalid());
        }
        let native_record = serde_json::to_string(&record).map_err(|_| invalid())?;
        if native_record.len() > RECORD_LIMIT {
            return Err(invalid());
        }
        let scope = record.scope();
        let manifest = Manifest {
            schema_version: 2,
            policy_version: POLICY_VERSION,
            id: random_id()?,
            connection_id: context.owner(),
            user: current_user()?,
            source_snapshot: snapshot,
            root: scope.root.to_str().ok_or_else(invalid)?.to_owned(),
            root_identity: format!("{:016x}:{:016x}", scope.identity.0, scope.identity.1),
            exclusions_count: scope.exclusions.len(),
            action: "recycle".into(),
            risk: "review".into(),
            created: now,
            expires: now.checked_add(PLAN_TTL).ok_or_else(invalid)?,
            entries,
            logical_bytes: logical.to_string(),
            potential_reclaim_bytes: reclaim.to_string(),
            immediate_free_bytes: "0".into(),
            native_record_hash: digest(native_record.as_bytes()),
            protection_fingerprint: protections.fingerprint().to_owned(),
        };
        let json = serde_json::to_string(&manifest).map_err(|_| invalid())?;
        if json.len() > MANIFEST_LIMIT {
            return Err(invalid());
        }
        let hash = digest(json.as_bytes());
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let count: i64 = tx.query_row("SELECT count(*) FROM agent_plans", [], |row| row.get(0))?;
        if count >= 10_000 {
            return Err(Error::Invalid("Plan retention limit reached.".into()));
        }
        tx.execute("INSERT INTO agent_plans(id,owner,manifest,hash,native_record,binding,state) VALUES(?1,?2,?3,?4,?5,?6,'draft')",
            params![manifest.id, context.owner(), json, hash, native_record, context.binding()?])?;
        audit(&tx, &manifest.id, now, "draft")?;
        tx.commit()?;
        Ok(View {
            manifest,
            hash,
            state: State::Draft,
            approval_expires: None,
            result: None,
        })
    }

    pub fn get(&mut self, context: &PlanContext<'_>, id: &str, now: i64) -> Result<View> {
        context.check(Capability::Metadata)?;
        Ok(self.load(context, id, now)?.view)
    }

    pub fn request(&mut self, context: &PlanContext<'_>, id: &str, now: i64) -> Result<View> {
        context.check(Capability::RequestApproval)?;
        let stored = self.load(context, id, now)?;
        if !matches!(stored.view.state, State::Draft | State::AwaitingApproval) {
            return Err(invalid());
        }
        self.validate_material(context, &stored, now)?;
        if stored.view.state == State::Draft {
            self.change(
                &stored.view,
                State::AwaitingApproval,
                now,
                "awaiting_approval",
            )?;
        }
        Ok(self.load(context, id, now)?.view)
    }

    /// Human desktop only, after showing this exact hash and selection. Never dispatch as an agent tool.
    pub fn approve(
        &mut self,
        context: &PlanContext<'_>,
        id: &str,
        hash: &str,
        human: &HumanAuthority,
        now: i64,
    ) -> Result<View> {
        context.check(Capability::Metadata)?;
        human.check_directory(self.directory.identity())?;
        let stored = self.load(context, id, now)?;
        if stored.view.state != State::AwaitingApproval || stored.view.hash != hash {
            return Err(invalid());
        }
        self.validate_material(context, &stored, now)?;
        let expires = now
            .checked_add(APPROVAL_TTL)
            .ok_or_else(invalid)?
            .min(stored.view.manifest.expires);
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if tx.execute("UPDATE agent_plans SET state='approved',approval_issuer=?1,approval_expires=?2 WHERE id=?3 AND owner=?4 AND hash=?5 AND state='awaiting_approval'",
            params![human.issuer(), expires, id, context.owner(), hash])? != 1 { return Err(invalid()); }
        audit(&tx, id, now, "approved")?;
        human.grant(id, hash, context.owner(), &stored.binding, now, expires)?;
        if let Err(error) = tx.commit() {
            human.forget(id)?;
            return Err(error.into());
        }
        Ok(self.load(context, id, now)?.view)
    }

    /// Returns one opaque ticket. Repeated calls observe the state instead of repeating I/O.
    pub fn begin(
        &mut self,
        context: &PlanContext<'_>,
        id: &str,
        human: &HumanAuthority,
        now: i64,
    ) -> Result<Begin> {
        context.check(Capability::ExecuteApproved)?;
        human.check_directory(self.directory.identity())?;
        let stored = self.load(context, id, now)?;
        let view = &stored.view;
        if view.state.terminal() || view.state == State::Executing {
            return Ok(Begin::Existing(view.state));
        }
        if view.state != State::Approved {
            return Err(Error::NeedsHumanApproval);
        }
        self.validate_material(context, &stored, now)?;
        let nonce = random_id()?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (current_state, current_hash): (String, String) = tx.query_row(
            "SELECT state,hash FROM agent_plans WHERE id=?1 AND owner=?2",
            params![id, context.owner()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        if current_hash != view.hash {
            return Err(invalid());
        }
        let current_state: State = serde_json::from_value(serde_json::Value::String(current_state))
            .map_err(|_| invalid())?;
        if current_state.terminal() || current_state == State::Executing {
            return Ok(Begin::Existing(current_state));
        }
        if current_state != State::Approved {
            return Err(Error::NeedsHumanApproval);
        }
        human.consume(id, &view.hash, context.owner(), &stored.binding, now)?;
        if tx.execute("UPDATE agent_plans SET state='executing',execution_nonce=?1 WHERE id=?2 AND owner=?3 AND hash=?4 AND state='approved' AND approval_issuer=?5 AND approval_expires>?6",
            params![nonce, id, context.owner(), view.hash, human.issuer(), now])? != 1 { return Err(Error::NeedsHumanApproval); }
        audit(&tx, id, now, "executing")?;
        tx.commit()?;
        Ok(Begin::Started(ExecutionLease {
            id: id.to_owned(),
            hash: view.hash.clone(),
            owner: context.owner(),
            nonce,
            directory: self.directory.identity(),
            #[cfg(windows)]
            issuer: human.issuer().to_owned(),
            #[cfg(windows)]
            binding: stored.binding,
            #[cfg(windows)]
            admission_deadline: view.approval_expires.ok_or_else(invalid)?,
        }))
    }

    pub fn finish(
        &mut self,
        lease: ExecutionLease,
        state: State,
        result: serde_json::Value,
        now: i64,
    ) -> Result<()> {
        if lease.directory != self.directory.identity()
            || !matches!(
                state,
                State::Completed
                    | State::PartiallyCompleted
                    | State::Failed
                    | State::Cancelled
                    | State::Invalidated
            )
            || now < 0
        {
            return Err(invalid());
        }
        let result = serde_json::to_string(&result).map_err(|_| invalid())?;
        if result.len() > 65536 {
            return Err(invalid());
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if tx.execute("UPDATE agent_plans SET state=?1,result=?2,execution_nonce=NULL WHERE id=?3 AND owner=?4 AND hash=?5 AND execution_nonce=?6 AND state='executing'",
            params![state.as_str(), result, lease.id, lease.owner, lease.hash, lease.nonce])? != 1 { return Err(invalid()); }
        audit(&tx, &lease.id, now, state.as_str())?;
        tx.commit()?;
        Ok(())
    }

    /// Recovery never retries uncertain I/O. Grants from a previous desktop session cease to exist.
    pub fn recover(&mut self, human: &HumanAuthority, now: i64) -> Result<()> {
        human.check_directory(self.directory.identity())?;
        if now < 0 {
            return Err(invalid());
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut statement = tx.prepare("SELECT id,state FROM agent_plans WHERE state IN ('approved','executing') AND (approval_issuer IS NULL OR approval_issuer!=?1) LIMIT 10001")?;
        let rows = statement
            .query_map([human.issuer()], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        drop(statement);
        if rows.len() > 10_000 {
            return Err(invalid());
        }
        for (id, state) in rows {
            let (next, event, result) = if state == "executing" {
                (
                    "failed",
                    "recovery_required",
                    Some(r#"{"recovery_required":true,"retry_allowed":false}"#),
                )
            } else {
                ("awaiting_approval", "approval_session_ended", None)
            };
            tx.execute("UPDATE agent_plans SET state=?1,approval_issuer=NULL,approval_expires=NULL,execution_nonce=NULL,result=?2 WHERE id=?3", params![next,result,id])?;
            audit(&tx, &id, now, event)?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn audit(
        &mut self,
        context: &PlanContext<'_>,
        id: &str,
        after: i64,
        limit: usize,
    ) -> Result<Vec<AuditEvent>> {
        let policy = context.check(Capability::Metadata)?;
        if after < 0 || limit == 0 || limit > 500 {
            return Err(invalid());
        }
        // Authorization is shared with normal plan reads, without requiring file contents.
        self.read(context, id)?;
        let mut statement = self.connection.prepare("SELECT sequence,at,event FROM plan_audit WHERE plan_id=?1 AND sequence>?2 ORDER BY sequence LIMIT ?3")?;
        Ok(statement
            .query_map(
                params![id, after, limit.min(policy.limits().page_size) as i64],
                |row| {
                    Ok(AuditEvent {
                        sequence: row.get(0)?,
                        at: row.get(1)?,
                        event: row.get(2)?,
                    })
                },
            )?
            .collect::<std::result::Result<Vec<_>, _>>()?)
    }

    pub fn reject(
        &mut self,
        context: &PlanContext<'_>,
        id: &str,
        human: &HumanAuthority,
        now: i64,
    ) -> Result<View> {
        context.check(Capability::Metadata)?;
        human.check_directory(self.directory.identity())?;
        let stored = self.load(context, id, now)?;
        if !matches!(
            stored.view.state,
            State::Draft | State::AwaitingApproval | State::Approved
        ) {
            return Err(invalid());
        }
        human.forget(id)?;
        self.change(&stored.view, State::Rejected, now, "rejected")?;
        Ok(self.load(context, id, now)?.view)
    }

    fn read(&self, context: &PlanContext<'_>, id: &str) -> Result<Stored> {
        context.check_directory(self.directory.identity())?;
        if id.len() != 64 || !id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(denied());
        }
        let values = self.connection.query_row("SELECT manifest,hash,native_record,binding,state,approval_expires,result FROM agent_plans WHERE id=?1 AND owner=?2 AND length(manifest)<=?3 AND length(native_record)<=?4 AND length(hash)=64 AND length(binding)=64 AND length(state)<=32 AND (result IS NULL OR length(result)<=65536)",
            params![id,context.owner(),MANIFEST_LIMIT as i64,RECORD_LIMIT as i64], |row| Ok((row.get::<_,String>(0)?,row.get::<_,String>(1)?,row.get::<_,String>(2)?,row.get::<_,String>(3)?,row.get::<_,String>(4)?,row.get::<_,Option<i64>>(5)?,row.get::<_,Option<String>>(6)?))).optional()?.ok_or_else(denied)?;
        let (json, hash, native, binding, state, approval_expires, result) = values;
        let manifest: Manifest = serde_json::from_str(&json).map_err(|_| invalid())?;
        if hash != digest(json.as_bytes())
            || manifest.native_record_hash != digest(native.as_bytes())
            || manifest.id != id
            || manifest.schema_version != 2
            || manifest.policy_version != POLICY_VERSION
            || manifest.connection_id != context.owner()
            || manifest.user != current_user()?
            || binding != context.binding()?
            || manifest.action != "recycle"
            || manifest.risk != "review"
            || manifest.created < 0
            || manifest.expires != manifest.created.checked_add(PLAN_TTL).ok_or_else(invalid)?
            || manifest.source_snapshot <= 0
        {
            return Err(invalid());
        }
        let record: Record = serde_json::from_str(&native).map_err(|_| invalid())?;
        if manifest.protection_fingerprint.len() != 64
            || !manifest
                .protection_fingerprint
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(invalid());
        }
        let scope = record.scope();
        let _root = context.snapshot(&scope)?;
        let evidence = record.evidence()?;
        let (logical, reclaim) = totals(&evidence)?;
        if manifest.entries != evidence
            || manifest.logical_bytes != logical.to_string()
            || manifest.potential_reclaim_bytes != reclaim.to_string()
            || manifest.immediate_free_bytes != "0"
            || Path::new(&manifest.root) != scope.root
            || manifest.root_identity
                != format!("{:016x}:{:016x}", scope.identity.0, scope.identity.1)
            || manifest.exclusions_count != scope.exclusions.len()
        {
            return Err(invalid());
        }
        let state =
            serde_json::from_value(serde_json::Value::String(state)).map_err(|_| invalid())?;
        let result = result
            .map(|json| serde_json::from_str(&json).map_err(|_| invalid()))
            .transpose()?;
        Ok(Stored {
            view: View {
                manifest,
                hash,
                state,
                approval_expires,
                result,
            },
            record,
            binding,
        })
    }

    #[cfg(windows)]
    pub(super) fn execution_record(
        &self,
        context: &PlanContext<'_>,
        human: &HumanAuthority,
        lease: &ExecutionLease,
    ) -> Result<Record> {
        context.check(Capability::ExecuteApproved)?;
        human.check_directory(self.directory.identity())?;
        if lease.directory != self.directory.identity()
            || lease.owner != context.owner()
            || lease.issuer != human.issuer()
            || lease.binding != context.binding()?
            || now()? >= lease.admission_deadline
        {
            return Err(invalid());
        }
        let stored = self.read(context, &lease.id)?;
        self.validate_protection_binding(&stored)?;
        let current: bool = self.connection.query_row("SELECT EXISTS(SELECT 1 FROM agent_plans WHERE id=?1 AND owner=?2 AND hash=?3 AND execution_nonce=?4 AND state='executing')",params![lease.id,lease.owner,lease.hash,lease.nonce],|row| row.get(0))?;
        if !current || stored.view.hash != lease.hash || stored.view.state != State::Executing {
            return Err(invalid());
        }
        Ok(stored.record)
    }

    #[cfg(windows)]
    pub(super) fn preflight_ready(
        &mut self,
        context: &PlanContext<'_>,
        human: &HumanAuthority,
        lease: &ExecutionLease,
    ) -> Result<()> {
        self.execution_record(context, human, lease)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        audit(&tx, &lease.id, now()?, "preflight_ready")?;
        tx.commit()?;
        Ok(())
    }

    fn load(&mut self, context: &PlanContext<'_>, id: &str, now: i64) -> Result<Stored> {
        let mut stored = self.read(context, id)?;
        if now < stored.view.manifest.created {
            return Err(invalid());
        }
        if !stored.view.state.terminal()
            && stored.view.state != State::Executing
            && self.validate_protection_binding(&stored).is_err()
        {
            self.change(&stored.view, State::Invalidated, now, "protections_changed")?;
            stored.view.state = State::Invalidated;
            stored.view.approval_expires = None;
        }
        let view = &stored.view;
        if !view.state.terminal()
            && view.state != State::Executing
            && (now >= view.manifest.expires
                || (view.state == State::Approved
                    && view.approval_expires.is_none_or(|expires| now >= expires)))
        {
            self.change(view, State::Expired, now, "expired")?;
            stored.view.state = State::Expired;
            stored.view.approval_expires = None;
        }
        Ok(stored)
    }

    fn validate_protection_binding(&self, stored: &Stored) -> Result<()> {
        let protections = crate::metadata::Store::open(self.directory.path())?.protections()?;
        if stored.view.manifest.protection_fingerprint != protections.fingerprint() {
            return Err(invalid());
        }
        stored.record.check_protections(&protections)
    }

    fn validate_material(
        &mut self,
        context: &PlanContext<'_>,
        stored: &Stored,
        now: i64,
    ) -> Result<()> {
        let checked = (|| {
            self.validate_protection_binding(stored)?;
            let entries = &stored.view.manifest.entries;
            let _pins = entries
                .iter()
                .map(|entry| context.path(Path::new(&entry.path)))
                .collect::<Result<Vec<_>>>()?;
            stored.record.validate(self.directory.identity())
        })();
        if checked.is_err() {
            self.change(&stored.view, State::Invalidated, now, "invalidated")?;
            return Err(invalid());
        }
        checked
    }

    fn change(&mut self, view: &View, state: State, now: i64, event: &str) -> Result<()> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if tx.execute("UPDATE agent_plans SET state=?1,approval_expires=NULL,approval_issuer=NULL WHERE id=?2 AND owner=?3 AND hash=?4 AND state=?5", params![state.as_str(),view.manifest.id,view.manifest.connection_id,view.hash,view.state.as_str()])? != 1 { return Err(invalid()); }
        audit(&tx, &view.manifest.id, now, event)?;
        tx.commit()?;
        Ok(())
    }
}

fn audit(connection: &Connection, id: &str, now: i64, event: &str) -> Result<()> {
    connection.execute(
        "INSERT INTO plan_audit(plan_id,at,event) VALUES(?1,?2,?3)",
        params![id, now, event],
    )?;
    Ok(())
}

fn totals(entries: &[FileEvidence]) -> Result<(u64, u64)> {
    if entries.is_empty() || entries.len() > 500 {
        return Err(invalid());
    }
    entries
        .iter()
        .try_fold((0u64, 0u64), |(logical, reclaim), entry| {
            let size = entry.logical_bytes.parse::<u64>().map_err(|_| invalid())?;
            let allocation = entry
                .allocated_bytes
                .parse::<u64>()
                .map_err(|_| invalid())?;
            Ok((
                logical.checked_add(size).ok_or_else(invalid)?,
                reclaim
                    .checked_add(if entry.hard_links == 1 { allocation } else { 0 })
                    .ok_or_else(invalid)?,
            ))
        })
}
