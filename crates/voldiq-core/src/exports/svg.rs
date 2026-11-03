mod validate;

use super::{
    ReportFilter, ReportItems, ReportSelection, StreamOutcome, StreamProgress,
    stream::{Record, walk},
};
use crate::{
    Error, NodeId, Result,
    cloud::CloudFilter,
    index::{Index, Query},
    workbench::{Color, Metric, Representation},
};
use serde::{Deserialize, Serialize};
use std::{
    io::Write,
    sync::atomic::{AtomicBool, Ordering},
};

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SvgCamera {
    pub x: f64,
    pub y: f64,
    pub scale: f64,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SvgHeightMetric {
    Modified,
    Files,
    Depth,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SvgHeight {
    pub metric: SvgHeightMetric,
    pub relief: f64,
    pub maximum: f64,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SvgView {
    pub representation: Representation,
    pub metric: Metric,
    pub color: Color,
    pub color_maximum: f64,
    pub reference_time: f64,
    pub dark: bool,
    pub depth: u8,
    pub width: f64,
    pub height: f64,
    pub camera: SvgCamera,
    pub detail: bool,
    pub city_height: Option<SvgHeight>,
    pub lens: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SvgRequest {
    pub scope: NodeId,
    pub cloud: Option<CloudFilter>,
    #[serde(default)]
    pub highlight: ReportFilter,
    pub view: SvgView,
    #[serde(default)]
    pub include_paths: bool,
    pub drawing: String,
}

fn invalid() -> Error {
    Error::Invalid("Unsupported or invalid storage-map SVG drawing.".into())
}

impl SvgRequest {
    fn validate(&self, index: &Index) -> Result<()> {
        let view = &self.view;
        if self.drawing.len() > 8 * 1024 * 1024
            || ![view.color_maximum, view.reference_time]
                .iter()
                .all(|v| v.is_finite() && *v >= 0.0)
            || !(1..=6).contains(&view.depth)
            || matches!(
                view.representation,
                Representation::Table | Representation::Grid
            )
            || ![view.width, view.height]
                .iter()
                .all(|v| v.is_finite() && *v > 0.0 && *v <= 8192.0)
            || ![view.camera.x, view.camera.y]
                .iter()
                .all(|v| v.is_finite() && v.abs() <= 10_000_000.0)
            || !view.camera.scale.is_finite()
            || view.camera.scale <= 0.0
            || view.camera.scale > 10_000.0
            || (view.representation == Representation::City) != view.city_height.is_some()
            || (view.lens && view.representation != Representation::Treemap)
            || view.city_height.as_ref().is_some_and(|v| {
                !v.maximum.is_finite()
                    || v.maximum < 0.0
                    || !v.relief.is_finite()
                    || v.relief <= 0.0
                    || v.relief > 8.0
            })
            || self.highlight.parent.is_some()
            || self
                .highlight
                .scope
                .is_some_and(|scope| scope != self.scope)
            || self
                .highlight
                .cloud
                .is_some_and(|cloud| Some(cloud) != self.cloud)
        {
            return Err(invalid());
        }
        let filter = &self.highlight;
        let query = Query {
            scope: Some(self.scope),
            text: filter.text.clone(),
            expression: filter.expression.clone(),
            extension: filter.extension.clone(),
            without_extension: filter.without_extension,
            min_size: filter.min_size,
            modified_before: filter.modified_before,
            cloud: self.cloud,
            ..Default::default()
        };
        index.validate_query_scope(&query.compile()?)?;
        index.entry(self.scope)?;
        Ok(())
    }
}

/// Client-rendered vectors are illustrations, not attested index quantities or file-operation authority.
/// Context comes from the captured core index; search highlights never become area filters here.
pub fn stream_svg(
    index: &Index,
    request: &SvgRequest,
    writer: &mut impl Write,
    cancelled: &AtomicBool,
    mut progress: impl FnMut(StreamProgress),
) -> Result<StreamOutcome> {
    let mut last = StreamProgress {
        inspected: 0,
        rows: 0,
        total_entries: index.entries.len().saturating_sub(1),
    };
    if cancelled.load(Ordering::Relaxed) {
        return Ok(StreamOutcome::Cancelled(last));
    }
    request.validate(index)?;
    let selection = ReportSelection {
        items: ReportItems::Files,
        filter: ReportFilter {
            scope: Some(request.scope),
            cloud: request.cloud,
            ..Default::default()
        },
        include_paths: request.include_paths,
    };
    let mut context = serde_json::Value::Null;
    let outcome = walk(
        index,
        &selection,
        cancelled,
        |state| {
            last = state;
            progress(state);
        },
        |record| {
            if let Record::Scope(value) = record {
                context = value;
            }
            Ok(true)
        },
    )?;
    if matches!(outcome, StreamOutcome::Cancelled(_)) {
        return Ok(outcome);
    }
    context["format"] = "svg".into();
    context["scope_node"] = request.scope.0.into();
    context["scope_incomplete"] = index.entry(request.scope)?.incomplete.into();
    context["scope_file_matches"] = last.rows.into();
    context["highlight_filter"] =
        super::filter_metadata(&request.highlight, request.include_paths)?;
    context["area_filter_role"]="Cloud selection controls represented bytes; search properties highlight the recorded view without changing its area scale.".into();
    context["client_declared_view"] = serde_json::to_value(&request.view).map_err(|_| invalid())?;
    context["graphic_authority"]="Client-rendered illustration, not attested quantities, a fresh scan, unique resident usage, reclaimable space or file-operation authority. Shapes may be grouped or outside the captured viewport; fonts are not embedded. Indexed scope counts are not counts of drawn shapes. Hover/lasso are not exported.".into();
    if request.include_paths {
        let Some(details) =
            index.details_cancellable(request.scope, || cancelled.load(Ordering::Relaxed))?
        else {
            return Ok(StreamOutcome::Cancelled(last));
        };
        context["scope_path"] = details.path.into();
        context["scope_path_exact"] = details.path_exact.into();
    } else {
        context["scope_path"] = serde_json::Value::Null;
        context["scope_path_exact"] = serde_json::Value::Null;
    }
    let Some(bytes) = validate::encode(&request.drawing, &request.view, &context, cancelled)?
    else {
        return Ok(StreamOutcome::Cancelled(last));
    };
    for chunk in bytes.chunks(64 * 1024) {
        if cancelled.load(Ordering::Relaxed) {
            return Ok(StreamOutcome::Cancelled(last));
        }
        writer.write_all(chunk)?;
    }
    Ok(if cancelled.load(Ordering::Relaxed) {
        StreamOutcome::Cancelled(last)
    } else {
        StreamOutcome::Complete(last)
    })
}
