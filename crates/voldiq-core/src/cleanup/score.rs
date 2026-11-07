use crate::{Entry, Error, Result};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Weights {
    pub size: u8,
    pub age: u8,
    pub regeneration: u8,
    pub duplication: u8,
    pub provenance: u8,
    pub recovery: u8,
}

impl Default for Weights {
    fn default() -> Self {
        Self {
            size: 25,
            age: 15,
            regeneration: 20,
            duplication: 15,
            provenance: 15,
            recovery: 10,
        }
    }
}

pub struct Model {
    weights: [u8; 6],
    total: u32,
}

impl Model {
    pub fn new(weights: Weights) -> Result<Self> {
        let weights = [
            weights.size,
            weights.age,
            weights.regeneration,
            weights.duplication,
            weights.provenance,
            weights.recovery,
        ];
        let total = weights.iter().copied().map(u32::from).sum();
        if total == 0 || weights.iter().any(|&weight| weight > 100) {
            return Err(Error::Invalid(
                "Use weights from 0 to 100 with at least one nonzero weight.".into(),
            ));
        }
        Ok(Self { weights, total })
    }

    pub(super) fn points(&self, facts: Facts) -> u16 {
        facts
            .signals
            .iter()
            .zip(self.weights)
            .map(|(signal, weight)| self.contribution(signal.value, weight))
            .sum()
    }

    pub(super) fn explain(&self, facts: Facts) -> Assessment {
        let mut observed_weight = 0;
        let mut points = 0;
        let signals = std::array::from_fn(|at| {
            let signal = facts.signals[at];
            let weight = self.weights[at];
            if signal.value.is_some() {
                observed_weight += u32::from(weight);
            }
            let contribution = self.contribution(signal.value, weight);
            points += contribution;
            Contribution {
                evidence: signal.evidence,
                value: signal.value,
                weight,
                points: contribution,
            }
        });
        Assessment {
            points,
            coverage: (observed_weight * 100 / self.total) as u8,
            signals,
        }
    }

    fn contribution(&self, value: Option<u8>, weight: u8) -> u16 {
        (u32::from(value.unwrap_or(0)) * u32::from(weight) * 100 / self.total) as u16
    }
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum Evidence {
    IndexedLogical,
    IndexedModified,
    BrowserHttpPayload,
    ScopedRecipe,
    ExactHash,
    IndependentLocalCopy,
    Unavailable,
}

#[derive(Clone, Copy)]
struct Signal {
    evidence: Evidence,
    value: Option<u8>,
}

impl Signal {
    const UNKNOWN: Self = Self {
        evidence: Evidence::Unavailable,
        value: None,
    };

    fn observed(evidence: Evidence, value: u8) -> Self {
        Self {
            evidence,
            value: Some(value),
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct Facts {
    signals: [Signal; 6],
}

impl Facts {
    pub(super) fn candidate(
        entry: &Entry,
        now: u64,
        browser: bool,
        duplicate: bool,
        copy: bool,
    ) -> Self {
        // Each doubling at or above 64 KiB adds six percent until the size signal caps.
        let size = (entry.logical / (64 * 1024))
            .checked_ilog2()
            .map_or(0, |band| ((band + 1) * 6).min(100) as u8);
        let age = entry
            .modified
            .and_then(|modified| now.checked_sub(modified))
            .map_or(Signal::UNKNOWN, |seconds| {
                Signal::observed(
                    Evidence::IndexedModified,
                    ((seconds / 86400).min(365) * 100 / 365) as u8,
                )
            });
        let available = |fact, evidence| {
            if fact {
                Signal::observed(evidence, 100)
            } else {
                Signal::UNKNOWN
            }
        };
        Self {
            signals: [
                Signal::observed(Evidence::IndexedLogical, size),
                age,
                available(browser, Evidence::BrowserHttpPayload),
                available(duplicate, Evidence::ExactHash),
                Signal::observed(Evidence::ScopedRecipe, 100),
                available(copy, Evidence::IndependentLocalCopy),
            ],
        }
    }
}

#[derive(Serialize)]
struct Contribution {
    evidence: Evidence,
    value: Option<u8>,
    weight: u8,
    points: u16,
}

#[derive(Serialize)]
pub struct Assessment {
    pub points: u16,
    pub coverage: u8,
    signals: [Contribution; 6],
}
