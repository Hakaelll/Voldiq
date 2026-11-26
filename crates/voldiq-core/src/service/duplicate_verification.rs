use super::AgentService;
use crate::{NodeId, Result, agents::denied, duplicates::Verification};
use std::sync::atomic::AtomicBool;

impl AgentService {
    pub fn verify_duplicates(
        &self,
        snapshot: i64,
        nodes: &[NodeId],
        cancel: &AtomicBool,
    ) -> Result<Option<Verification>> {
        #[cfg(windows)]
        {
            use crate::agents::Capability;
            use std::sync::atomic::Ordering;

            let registry = self.registry()?;
            let context = registry.plan_context(&self.principal, Capability::ReadContent)?;
            let (policy, binding) = context.bound_policy(Capability::ReadContent)?;
            if nodes.len() / 2 > policy.limits().page_size {
                return Err(denied());
            }
            if cancel.load(Ordering::Relaxed) {
                return Ok(None);
            }
            let index = self.index(snapshot)?;
            let _root = registry.authorize_index(&self.principal, &index)?;
            let mut notes = crate::metadata::Store::open(self.directory.directory())?;
            let protections = notes.protections()?;
            let still_authorized = || {
                if context.binding()? != binding
                    || notes.protection_fingerprint()? != protections.fingerprint()
                {
                    return Err(denied());
                }
                Ok(())
            };
            crate::duplicates::verify_selection(
                &index,
                nodes,
                &protections,
                cancel,
                |node| {
                    still_authorized()?;
                    registry.authorize_index_entry(
                        &self.principal,
                        Capability::ReadContent,
                        &index,
                        node,
                    )
                },
                || {
                    self.validate_index(&registry, &index)?;
                    still_authorized()
                },
            )
        }
        #[cfg(not(windows))]
        {
            let _ = (snapshot, nodes, cancel);
            Err(denied())
        }
    }
}
