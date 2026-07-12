#![cfg(target_os = "windows")]

use std::collections::{HashMap, HashSet};
use std::fs;
use std::mem::{size_of, MaybeUninit};
use std::path::PathBuf;
use std::sync::Mutex;

use crate::diagnostics;
use monarch::{DisplayBackend, DisplayId, DisplayInfo, Layout, ManagerError};
use serde::{Deserialize, Serialize};

use super::apply::{
    active_color_state_signature, apply_layout_against_snapshot, capture_sdr_gamma_ramps,
    force_topology_extend, gamma_ramp_looks_identity,
    reapply_color_calibration_for_active_with_cached_sdr, GammaRampKey, GammaRampWords,
};
use super::enumerate::{query_active_only_topology, query_active_topology, snapshot_from_raw};
use super::win32_types::{luid_to_u64, RawTopologySnapshot, TopologySnapshot};

const PERSISTED_RAW_SNAPSHOT_VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize)]
struct PersistedRawSnapshot {
    version: u32,
    path_struct_size: usize,
    mode_struct_size: usize,
    paths: Vec<Vec<u8>>,
    modes: Vec<Vec<u8>>,
}

#[derive(Default)]
struct BackendCache {
    last_snapshot: Option<TopologySnapshot>,
    last_layout: Option<Layout>,
    last_displays: Vec<DisplayInfo>,
    sdr_gamma_cache: HashMap<GammaRampKey, GammaRampWords>,
    /// Set by `invalidate_cache`: the next refresh re-seeds detached-display knowledge from the
    /// persisted snapshot (when it still matches this boot), so an invalidation does not lose a
    /// detached display whenever QDC_DATABASE_CURRENT enrichment fails afterwards.
    reseed_persisted: bool,
}

#[derive(Default)]
pub struct WindowsDisplayBackend {
    cache: Mutex<BackendCache>,
}

impl WindowsDisplayBackend {
    pub fn new() -> Result<Self, ManagerError> {
        let backend = Self::default();
        let mut persist_now = true;
        let snapshot = {
            let fresh = query_active_topology()?;
            match load_persisted_raw_snapshot() {
                Some(persisted_raw) if persisted_raw.paths.len() > fresh.raw.paths.len() => {
                    match merge_persisted_raw_for_fresh(&fresh, &persisted_raw) {
                        Some(merged) => merged,
                        None => {
                            // The persisted snapshot no longer matches this boot (e.g. adapter
                            // LUID churn after a reboot). Keep the file on disk: overwriting it
                            // here would destroy the only record of a detached display's path.
                            // It is replaced after the next successful apply.
                            persist_now = false;
                            diagnostics::log(
                                "topology_persist:skip:persisted_snapshot_rejected_at_startup",
                            );
                            fresh
                        }
                    }
                }
                _ => fresh,
            }
        };
        let initial_sdr_ramps = capture_sdr_gamma_ramps(&snapshot);
        let raw_to_persist = snapshot.raw.clone();
        let mut cache = backend
            .cache
            .lock()
            .map_err(|_| ManagerError::Backend("windows backend cache poisoned".to_string()))?;
        cache.last_layout = Some(snapshot.layout.clone());
        cache.last_displays = snapshot.displays.clone();
        cache.last_snapshot = Some(snapshot);
        merge_sdr_gamma_cache(&mut cache.sdr_gamma_cache, initial_sdr_ramps);
        drop(cache);
        if persist_now {
            best_effort_persist_raw_snapshot(&raw_to_persist);
        }
        Ok(backend)
    }

    /// Drop every cached snapshot/layout/display entry (the SDR gamma cache is kept) so the next
    /// refresh rebuilds state from a fresh enriched enumeration. Used after system resume, when
    /// stale adapter LUIDs and transient EDID read failures would otherwise pollute the cache.
    pub fn invalidate_cache(&self) -> Result<(), ManagerError> {
        let mut cache = self
            .cache
            .lock()
            .map_err(|_| ManagerError::Backend("windows backend cache poisoned".to_string()))?;
        cache.last_snapshot = None;
        cache.last_layout = None;
        cache.last_displays.clear();
        cache.reseed_persisted = true;
        drop(cache);
        diagnostics::log("backend_cache:invalidated");
        Ok(())
    }

    /// Best-effort recovery used by the manager before rejecting a profile/restore whose enabled
    /// outputs cannot be resolved against the current enumeration (e.g. detached before a reboot
    /// and QDC_DATABASE_CURRENT enrichment failed to surface it): force a topology extend — the
    /// same action as the user's manual Win+P workaround — so Windows recreates the paths, then
    /// invalidate the cache so the next query re-enumerates fresh. Extend failures are swallowed
    /// on purpose: the caller's strict re-validation reports the real error.
    pub fn prepare_attach_targets(&self, desired: &Layout) -> Result<(), ManagerError> {
        let snapshot = query_active_topology()?;
        let remapped = remap_layout_display_ids_for_snapshot(
            desired,
            &snapshot.layout,
            &raw_path_connectors(&snapshot.raw),
        );
        let current_ids: HashSet<DisplayId> = snapshot
            .layout
            .outputs
            .iter()
            .map(|output| output.display_id.clone())
            .collect();
        let needs_extend = remapped
            .outputs
            .iter()
            .any(|output| output.enabled && !current_ids.contains(&output.display_id));
        if !needs_extend {
            return Ok(());
        }

        diagnostics::log("prepare_attach_targets:force_extend");
        if let Err(error) = force_topology_extend() {
            diagnostics::log(format!("prepare_attach_targets:extend_failed:{error}"));
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(700));
        self.invalidate_cache()
    }

    fn refresh_active(&self) -> Result<(), ManagerError> {
        let mut snapshot = query_active_topology()?;
        let fresh_connectors = raw_path_connectors(&snapshot.raw);
        let mut cache = self
            .cache
            .lock()
            .map_err(|_| ManagerError::Backend("windows backend cache poisoned".to_string()))?;

        if cache.reseed_persisted {
            cache.reseed_persisted = false;
            // The cache was invalidated (resume): re-seed detached-display entries from the
            // persisted snapshot so they survive a failed enrichment. The merge already rejects
            // snapshots with stale adapter LUIDs, preserving the point of the invalidation.
            if let Some(persisted_raw) = load_persisted_raw_snapshot() {
                if persisted_raw.paths.len() > snapshot.raw.paths.len() {
                    if let Some(merged) = merge_persisted_raw_for_fresh(&snapshot, &persisted_raw) {
                        diagnostics::log("backend_cache:reseeded_from_persisted_snapshot");
                        snapshot = merged;
                    }
                }
            }
        }

        cache.last_snapshot = Some(merge_snapshot_for_cache(
            cache.last_snapshot.as_ref(),
            snapshot.clone(),
        ));
        cache.last_layout = Some(merge_layout_with_fresh(
            cache.last_layout.as_ref(),
            &snapshot.layout,
            &fresh_connectors,
        ));
        cache.last_displays =
            merge_displays_with_fresh(&cache.last_displays, &snapshot.displays, &fresh_connectors);
        Ok(())
    }

    pub fn reapply_color_calibration(&self) -> Result<(), ManagerError> {
        let cached_sdr = {
            let cache = self
                .cache
                .lock()
                .map_err(|_| ManagerError::Backend("windows backend cache poisoned".to_string()))?;
            cache.sdr_gamma_cache.clone()
        };

        reapply_color_calibration_for_active_with_cached_sdr(&cached_sdr)?;
        let refreshed_snapshot = query_active_topology()?;

        let mut cache = self
            .cache
            .lock()
            .map_err(|_| ManagerError::Backend("windows backend cache poisoned".to_string()))?;
        merge_sdr_gamma_cache(
            &mut cache.sdr_gamma_cache,
            capture_sdr_gamma_ramps(&refreshed_snapshot),
        );
        Ok(())
    }

    pub fn color_state_signature(&self) -> Result<Option<String>, ManagerError> {
        let snapshot = query_active_topology()?;
        Ok(Some(active_color_state_signature(&snapshot)))
    }
}

fn merge_snapshot_for_cache(
    previous: Option<&TopologySnapshot>,
    fresh: TopologySnapshot,
) -> TopologySnapshot {
    let Some(previous) = previous else {
        return fresh;
    };

    // Preserve an older raw snapshot when it still covers the currently active outputs and
    // contains more paths. This keeps a recently-detached display path available for re-attach.
    if previous.raw.paths.len() > fresh.raw.paths.len()
        && raw_covers_active_outputs_raw(&previous.raw, &fresh.layout)
    {
        let mut merged = fresh;
        merged.raw = previous.raw.clone();
        return merged;
    }

    fresh
}

fn merge_persisted_raw_for_fresh(
    fresh: &TopologySnapshot,
    persisted_raw: &RawTopologySnapshot,
) -> Option<TopologySnapshot> {
    if !raw_covers_active_outputs_raw(persisted_raw, &fresh.layout) {
        return None;
    }

    let persisted_snapshot = snapshot_from_raw(persisted_raw.clone()).ok()?;

    let fresh_connectors = raw_path_connectors(&fresh.raw);
    Some(TopologySnapshot {
        raw: persisted_snapshot.raw,
        layout: merge_layout_with_fresh(
            Some(&persisted_snapshot.layout),
            &fresh.layout,
            &fresh_connectors,
        ),
        displays: merge_displays_with_fresh(
            &persisted_snapshot.displays,
            &fresh.displays,
            &fresh_connectors,
        ),
    })
}

fn raw_covers_active_outputs_raw(raw: &RawTopologySnapshot, layout: &Layout) -> bool {
    layout
        .outputs
        .iter()
        .filter(|output| output.enabled)
        .all(|output| {
            raw.paths.iter().any(|path| {
                let adapter_luid = ((path.targetInfo.adapterId.HighPart as i64 as u64) << 32)
                    | (path.targetInfo.adapterId.LowPart as u64);
                adapter_luid == output.display_id.adapter_luid
                    && path.targetInfo.id == output.display_id.target_id
            })
        })
}

fn raw_path_connectors(raw: &RawTopologySnapshot) -> HashSet<(u64, u32)> {
    raw.paths
        .iter()
        .map(|path| {
            (
                luid_to_u64(
                    path.targetInfo.adapterId.HighPart,
                    path.targetInfo.adapterId.LowPart,
                ),
                path.targetInfo.id,
            )
        })
        .collect()
}

/// A cached entry is a stale duplicate when the same physical monitor (same EDID hash) shows up
/// in the fresh snapshot under a different (adapter_luid, target_id) AND the cached connector's
/// path no longer exists in the fresh snapshot at all (e.g. adapter LUID churn after resume or
/// reboot). Twin monitors with identical EDIDs are protected: both connectors keep existing.
fn cached_id_is_stale_duplicate<'a>(
    cached: &DisplayId,
    mut fresh_ids: impl Iterator<Item = &'a DisplayId>,
    fresh_connectors: &HashSet<(u64, u32)>,
) -> bool {
    let Some(edid_hash) = cached.edid_hash else {
        return false;
    };
    let cached_connector = (cached.adapter_luid, cached.target_id);
    if fresh_connectors.contains(&cached_connector) {
        return false;
    }
    fresh_ids.any(|fresh| {
        fresh.edid_hash == Some(edid_hash)
            && (fresh.adapter_luid, fresh.target_id) != cached_connector
    })
}

fn merge_layout_with_fresh(
    previous: Option<&Layout>,
    fresh: &Layout,
    fresh_connectors: &HashSet<(u64, u32)>,
) -> Layout {
    let Some(previous) = previous else {
        return fresh.clone();
    };

    let mut outputs: Vec<monarch::OutputConfig> = Vec::new();
    for cached in &previous.outputs {
        if cached_id_is_stale_duplicate(
            &cached.display_id,
            fresh.outputs.iter().map(|output| &output.display_id),
            fresh_connectors,
        ) {
            continue;
        }

        let cached_connector = (cached.display_id.adapter_luid, cached.display_id.target_id);
        let next = if let Some(active) = fresh.outputs.iter().find(|active| {
            (active.display_id.adapter_luid, active.display_id.target_id) == cached_connector
        }) {
            // Same connector: the fresh data wins. Keep a known EDID hash when the fresh read
            // transiently failed so the display identity stays stable across the glitch.
            let mut next = active.clone();
            if next.display_id.edid_hash.is_none() {
                next.display_id.edid_hash = cached.display_id.edid_hash;
            }
            next
        } else {
            let mut inactive = cached.clone();
            inactive.enabled = false;
            inactive.primary = false;
            inactive
        };
        push_output_preferring_known_edid(&mut outputs, next);
    }
    for active in &fresh.outputs {
        let connector = (active.display_id.adapter_luid, active.display_id.target_id);
        if !outputs.iter().any(|output| {
            (output.display_id.adapter_luid, output.display_id.target_id) == connector
        }) {
            outputs.push(active.clone());
        }
    }

    let mut merged = Layout { outputs };
    if !merged
        .outputs
        .iter()
        .any(|output| output.enabled && output.primary)
    {
        if let Some(first) = merged.outputs.iter_mut().find(|output| output.enabled) {
            first.primary = true;
        }
    }
    merged
}

fn push_output_preferring_known_edid(
    outputs: &mut Vec<monarch::OutputConfig>,
    next: monarch::OutputConfig,
) {
    let connector = (next.display_id.adapter_luid, next.display_id.target_id);
    if let Some(existing) = outputs
        .iter_mut()
        .find(|output| (output.display_id.adapter_luid, output.display_id.target_id) == connector)
    {
        if existing.display_id.edid_hash.is_none() && next.display_id.edid_hash.is_some() {
            *existing = next;
        }
        return;
    }
    outputs.push(next);
}

fn merge_displays_with_fresh(
    previous: &[DisplayInfo],
    fresh: &[DisplayInfo],
    fresh_connectors: &HashSet<(u64, u32)>,
) -> Vec<DisplayInfo> {
    let mut merged: Vec<DisplayInfo> = Vec::new();
    for cached in previous {
        if cached_id_is_stale_duplicate(
            &cached.id,
            fresh.iter().map(|display| &display.id),
            fresh_connectors,
        ) {
            continue;
        }

        let cached_connector = (cached.id.adapter_luid, cached.id.target_id);
        let next = if let Some(active) = fresh
            .iter()
            .find(|active| (active.id.adapter_luid, active.id.target_id) == cached_connector)
        {
            let mut next = active.clone();
            if next.id.edid_hash.is_none() {
                next.id.edid_hash = cached.id.edid_hash;
            }
            next
        } else {
            let mut inactive = cached.clone();
            inactive.is_active = false;
            inactive.is_primary = false;
            inactive
        };
        push_display_preferring_known_edid(&mut merged, next);
    }
    for active in fresh {
        let connector = (active.id.adapter_luid, active.id.target_id);
        if !merged
            .iter()
            .any(|display| (display.id.adapter_luid, display.id.target_id) == connector)
        {
            merged.push(active.clone());
        }
    }
    merged.sort_by(|left, right| {
        left.friendly_name
            .cmp(&right.friendly_name)
            .then(left.id.target_id.cmp(&right.id.target_id))
    });
    merged
}

fn push_display_preferring_known_edid(displays: &mut Vec<DisplayInfo>, next: DisplayInfo) {
    let connector = (next.id.adapter_luid, next.id.target_id);
    if let Some(existing) = displays
        .iter_mut()
        .find(|display| (display.id.adapter_luid, display.id.target_id) == connector)
    {
        if existing.id.edid_hash.is_none() && next.id.edid_hash.is_some() {
            *existing = next;
        }
        return;
    }
    displays.push(next);
}

impl DisplayBackend for WindowsDisplayBackend {
    fn list_displays(&self) -> Result<Vec<DisplayInfo>, ManagerError> {
        self.refresh_active()?;
        let cache = self
            .cache
            .lock()
            .map_err(|_| ManagerError::Backend("windows backend cache poisoned".to_string()))?;
        Ok(cache.last_displays.clone())
    }

    fn get_layout(&self) -> Result<Layout, ManagerError> {
        self.refresh_active()?;
        let cache = self
            .cache
            .lock()
            .map_err(|_| ManagerError::Backend("windows backend cache poisoned".to_string()))?;
        cache
            .last_layout
            .clone()
            .ok_or_else(|| ManagerError::Backend("no cached layout available".to_string()))
    }

    fn apply_layout(&self, layout: Layout) -> Result<(), ManagerError> {
        layout.ensure_valid()?;
        diagnostics::log(format!(
            "topology_apply:start:outputs={}",
            layout.outputs.len()
        ));

        // Re-query the currently active topology so detach-only operations use a minimal base.
        // This reduces the chance of Windows re-touching unrelated outputs.
        let active_snapshot = query_active_topology()?;
        let needs_attach_paths = desired_enables_inactive_output(&layout, &active_snapshot.layout);

        let base_snapshot = if !needs_attach_paths {
            // Detach-only change: apply against a minimal, non-enriched active snapshot so
            // database-sourced paths are never fed into SetDisplayConfig (avoids spurious
            // error-87 risks mid-detach).
            query_active_only_topology()?
        } else {
            let cache = self
                .cache
                .lock()
                .map_err(|_| ManagerError::Backend("windows backend cache poisoned".to_string()))?;

            if let Some(cached) = cache.last_snapshot.clone() {
                if raw_covers_active_outputs_raw(&cached.raw, &active_snapshot.layout) {
                    cached
                } else {
                    active_snapshot.clone()
                }
            } else {
                active_snapshot.clone()
            }
        };

        let working_layout = remap_layout_display_ids_for_snapshot(
            &layout,
            &base_snapshot.layout,
            &raw_path_connectors(&active_snapshot.raw),
        );

        let missing_attach_outputs =
            enabled_outputs_missing_from_raw(&working_layout, &base_snapshot.raw);
        let (next_snapshot, applied_layout) = if !missing_attach_outputs.is_empty() {
            // The base snapshot has no path for these outputs, so flipping active flags would be
            // a silent no-op (SetDisplayConfig returns 0 on an unchanged active set). If the
            // display is currently connected, force an extend so Windows recreates its path and
            // retry once; otherwise fail with an actionable error instead of applying a no-op.
            if let Some(unavailable) = missing_attach_outputs
                .iter()
                .find(|output| !output_connected_in_snapshot(output, &active_snapshot))
            {
                let description = describe_output_for_error(unavailable, &base_snapshot);
                diagnostics::log(format!(
                    "topology_apply:error:attach_target_not_connected:{description}"
                ));
                return Err(ManagerError::Backend(format!(
                    "cannot attach display {description}: it is not currently connected to this system"
                )));
            }
            diagnostics::log("topology_apply:retry:reason=attach_paths_missing");
            recover_apply_with_topology_extend(&working_layout)?
        } else {
            match apply_layout_against_snapshot(&working_layout, &base_snapshot) {
                Ok(snapshot) => (snapshot, working_layout),
                Err(error) if is_set_display_invalid_parameter(&error) => {
                    diagnostics::log("topology_apply:retry:reason=setdisplayconfig_87");
                    recover_apply_with_topology_extend(&working_layout)?
                }
                Err(error) => {
                    diagnostics::log(format!("topology_apply:error:{error}"));
                    return Err(error);
                }
            }
        };
        let mut cache = self
            .cache
            .lock()
            .map_err(|_| ManagerError::Backend("windows backend cache poisoned".to_string()))?;
        let merged_snapshot = merge_snapshot_for_cache(Some(&base_snapshot), next_snapshot.clone());
        let raw_to_persist = merged_snapshot.raw.clone();
        cache.last_snapshot = Some(merged_snapshot);
        merge_sdr_gamma_cache(
            &mut cache.sdr_gamma_cache,
            capture_sdr_gamma_ramps(&next_snapshot),
        );

        let mut merged_layout = applied_layout;
        for output in &mut merged_layout.outputs {
            if let Some(active) = next_snapshot
                .layout
                .outputs
                .iter()
                .find(|active| active.display_id == output.display_id)
            {
                output.position = active.position.clone();
                output.resolution = active.resolution.clone();
                output.refresh_rate_mhz = active.refresh_rate_mhz;
                output.enabled = true;
                output.primary = active.primary;
            }
        }
        cache.last_layout = Some(merged_layout);

        let mut displays = cache.last_displays.clone();
        for display in &mut displays {
            if let Some(active) = next_snapshot.displays.iter().find(|d| d.id == display.id) {
                *display = active.clone();
            } else {
                display.is_active = false;
                display.is_primary = false;
            }
        }
        for active in &next_snapshot.displays {
            if !displays.iter().any(|d| d.id == active.id) {
                displays.push(active.clone());
            }
        }
        cache.last_displays = displays;
        drop(cache);
        best_effort_persist_raw_snapshot(&raw_to_persist);
        diagnostics::log("topology_apply:done");

        Ok(())
    }

    fn color_state_signature(&self) -> Result<Option<String>, ManagerError> {
        WindowsDisplayBackend::color_state_signature(self)
    }

    fn reapply_color_calibration(&self) -> Result<(), ManagerError> {
        WindowsDisplayBackend::reapply_color_calibration(self)
    }

    fn invalidate_cache(&self) -> Result<(), ManagerError> {
        WindowsDisplayBackend::invalidate_cache(self)
    }

    fn prepare_attach_targets(&self, desired: &Layout) -> Result<(), ManagerError> {
        WindowsDisplayBackend::prepare_attach_targets(self, desired)
    }
}

fn merge_sdr_gamma_cache(
    cache: &mut HashMap<GammaRampKey, GammaRampWords>,
    observed: HashMap<GammaRampKey, GammaRampWords>,
) {
    for (key, ramp) in observed {
        match cache.get(&key) {
            // Preserve a previous non-identity SDR ramp if the newly observed ramp looks like a
            // reset/default ramp (common after HDR transitions on some drivers).
            Some(existing)
                if !gamma_ramp_looks_identity(existing) && gamma_ramp_looks_identity(&ramp) => {}
            _ => {
                cache.insert(key, ramp);
            }
        }
    }
}

fn desired_enables_inactive_output(desired: &Layout, active_layout: &Layout) -> bool {
    desired.outputs.iter().any(|output| {
        output.enabled
            && !active_layout
                .outputs
                .iter()
                .any(|active| active.enabled && active.display_id == output.display_id)
    })
}

fn remap_layout_display_ids_for_snapshot(
    desired: &Layout,
    current: &Layout,
    enumerated_connectors: &HashSet<(u64, u32)>,
) -> Layout {
    let current_ids: HashSet<DisplayId> = current
        .outputs
        .iter()
        .map(|output| output.display_id.clone())
        .collect();

    if desired
        .outputs
        .iter()
        .all(|output| current_ids.contains(&output.display_id))
    {
        return desired.clone();
    }

    let mut remapped = desired.clone();
    let mut used: HashSet<DisplayId> = HashSet::new();
    for output in &remapped.outputs {
        if current_ids.contains(&output.display_id) {
            used.insert(output.display_id.clone());
        }
    }

    let mut current_by_edid: HashMap<u64, Vec<&monarch::OutputConfig>> = HashMap::new();
    for output in &current.outputs {
        if let Some(edid_hash) = output.display_id.edid_hash {
            current_by_edid.entry(edid_hash).or_default().push(output);
        }
    }

    for output in &mut remapped.outputs {
        if current_ids.contains(&output.display_id) {
            continue;
        }

        let mut replacement = None;

        if let Some(edid_hash) = output.display_id.edid_hash {
            let candidates = unique_unused_candidates(
                current_by_edid.get(&edid_hash).cloned().unwrap_or_default(),
                &used,
            );
            replacement = choose_remap_candidate(&candidates, enumerated_connectors)
                .map(|candidate| candidate.display_id.clone());
        }

        if replacement.is_none() && output.display_id.edid_hash.is_none() {
            // Never guess across adapters in the hash-less fallback: iGPU/dGPU pairs reuse the
            // same target id numbering, so a cross-adapter pick could hit the wrong monitor.
            let candidates = unique_unused_candidates_by_target_id(
                output.display_id.target_id,
                &current.outputs,
                &used,
            );
            if candidates_share_one_adapter(&candidates) {
                replacement = choose_remap_candidate(&candidates, enumerated_connectors)
                    .map(|candidate| candidate.display_id.clone());
            }
        }

        if let Some(next_id) = replacement {
            used.insert(next_id.clone());
            output.display_id = next_id;
        }
    }

    remapped
}

fn candidates_share_one_adapter(candidates: &[&monarch::OutputConfig]) -> bool {
    let mut adapters = candidates
        .iter()
        .map(|candidate| candidate.display_id.adapter_luid);
    let Some(first) = adapters.next() else {
        return true;
    };
    adapters.all(|adapter| adapter == first)
}

fn choose_remap_candidate<'a>(
    candidates: &[&'a monarch::OutputConfig],
    enumerated_connectors: &HashSet<(u64, u32)>,
) -> Option<&'a monarch::OutputConfig> {
    if candidates.is_empty() {
        return None;
    }
    if candidates.len() == 1 {
        return Some(candidates[0]);
    }

    // Deterministic tie-break for duplicate identities (e.g. a stale cached entry plus the same
    // physical monitor re-enumerated under a new adapter LUID after resume/reboot): prefer the
    // candidate whose connector is currently enumerated, then the single enabled one. Two active
    // identical twins stay ambiguous and are left unmapped.
    let enumerated: Vec<_> = candidates
        .iter()
        .copied()
        .filter(|candidate| {
            enumerated_connectors.contains(&(
                candidate.display_id.adapter_luid,
                candidate.display_id.target_id,
            ))
        })
        .collect();
    if enumerated.len() == 1 {
        return Some(enumerated[0]);
    }

    let pool = if enumerated.is_empty() {
        candidates.to_vec()
    } else {
        enumerated
    };
    let enabled: Vec<_> = pool
        .iter()
        .copied()
        .filter(|candidate| candidate.enabled)
        .collect();
    if enabled.len() == 1 {
        return Some(enabled[0]);
    }

    None
}

fn enabled_outputs_missing_from_raw<'a>(
    layout: &'a Layout,
    raw: &RawTopologySnapshot,
) -> Vec<&'a monarch::OutputConfig> {
    let connectors = raw_path_connectors(raw);
    layout
        .outputs
        .iter()
        .filter(|output| output.enabled)
        .filter(|output| {
            !connectors.contains(&(output.display_id.adapter_luid, output.display_id.target_id))
        })
        .collect()
}

fn output_connected_in_snapshot(
    output: &monarch::OutputConfig,
    snapshot: &TopologySnapshot,
) -> bool {
    if let Some(edid_hash) = output.display_id.edid_hash {
        return snapshot
            .displays
            .iter()
            .any(|display| display.id.edid_hash == Some(edid_hash));
    }
    snapshot.displays.iter().any(|display| {
        display.id.adapter_luid == output.display_id.adapter_luid
            && display.id.target_id == output.display_id.target_id
    })
}

fn describe_output_for_error(
    output: &monarch::OutputConfig,
    base_snapshot: &TopologySnapshot,
) -> String {
    let edid = output
        .display_id
        .edid_hash
        .map(|value| format!("{value:016x}"))
        .unwrap_or_else(|| "-".to_string());
    let friendly = base_snapshot
        .displays
        .iter()
        .find(|display| {
            display.id == output.display_id
                || (output.display_id.edid_hash.is_some()
                    && display.id.edid_hash == output.display_id.edid_hash)
        })
        .map(|display| format!("'{}' ", display.friendly_name))
        .unwrap_or_default();
    format!(
        "{friendly}(target_id={}, edid_hash={edid})",
        output.display_id.target_id
    )
}

fn recover_apply_with_topology_extend(
    working_layout: &Layout,
) -> Result<(TopologySnapshot, Layout), ManagerError> {
    force_topology_extend()?;
    std::thread::sleep(std::time::Duration::from_millis(700));
    let recovered_snapshot = query_active_topology()?;
    let retry_layout = remap_layout_display_ids_for_snapshot(
        working_layout,
        &recovered_snapshot.layout,
        &raw_path_connectors(&recovered_snapshot.raw),
    );
    let snapshot = apply_layout_against_snapshot(&retry_layout, &recovered_snapshot)?;
    Ok((snapshot, retry_layout))
}

fn unique_unused_candidates<'a>(
    candidates: Vec<&'a monarch::OutputConfig>,
    used: &HashSet<DisplayId>,
) -> Vec<&'a monarch::OutputConfig> {
    candidates
        .into_iter()
        .filter(|candidate| !used.contains(&candidate.display_id))
        .collect()
}

fn unique_unused_candidates_by_target_id<'a>(
    target_id: u32,
    current_outputs: &'a [monarch::OutputConfig],
    used: &HashSet<DisplayId>,
) -> Vec<&'a monarch::OutputConfig> {
    current_outputs
        .iter()
        .filter(|candidate| candidate.display_id.target_id == target_id)
        .filter(|candidate| !used.contains(&candidate.display_id))
        .collect()
}

fn is_set_display_invalid_parameter(error: &ManagerError) -> bool {
    matches!(
        error,
        ManagerError::Backend(message) if message.contains("SetDisplayConfig failed: 87")
    )
}

fn best_effort_persist_raw_snapshot(raw: &RawTopologySnapshot) {
    if let Err(err) = persist_raw_snapshot(raw) {
        eprintln!("Monarch persisted topology snapshot write failed: {err}");
    }
}

fn persist_raw_snapshot(raw: &RawTopologySnapshot) -> Result<(), ManagerError> {
    let payload = PersistedRawSnapshot {
        version: PERSISTED_RAW_SNAPSHOT_VERSION,
        path_struct_size: size_of::<windows::Win32::Devices::Display::DISPLAYCONFIG_PATH_INFO>(),
        mode_struct_size: size_of::<windows::Win32::Devices::Display::DISPLAYCONFIG_MODE_INFO>(),
        paths: raw.paths.iter().map(struct_to_bytes).collect(),
        modes: raw.modes.iter().map(struct_to_bytes).collect(),
    };

    let path = persisted_raw_snapshot_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            ManagerError::Backend(format!(
                "failed to create persisted snapshot directory: {err}"
            ))
        })?;
    }

    let body = serde_json::to_vec(&payload)
        .map_err(|err| ManagerError::Backend(format!("failed to encode snapshot: {err}")))?;
    fs::write(&path, body)
        .map_err(|err| ManagerError::Backend(format!("failed to write snapshot: {err}")))?;
    Ok(())
}

fn load_persisted_raw_snapshot() -> Option<RawTopologySnapshot> {
    let path = persisted_raw_snapshot_path();
    let body = fs::read(path).ok()?;
    let payload: PersistedRawSnapshot = serde_json::from_slice(&body).ok()?;
    if payload.version != PERSISTED_RAW_SNAPSHOT_VERSION {
        return None;
    }
    if payload.path_struct_size
        != size_of::<windows::Win32::Devices::Display::DISPLAYCONFIG_PATH_INFO>()
        || payload.mode_struct_size
            != size_of::<windows::Win32::Devices::Display::DISPLAYCONFIG_MODE_INFO>()
    {
        return None;
    }

    let mut paths = Vec::with_capacity(payload.paths.len());
    for bytes in payload.paths {
        paths.push(struct_from_bytes::<
            windows::Win32::Devices::Display::DISPLAYCONFIG_PATH_INFO,
        >(&bytes)?);
    }

    let mut modes = Vec::with_capacity(payload.modes.len());
    for bytes in payload.modes {
        modes.push(struct_from_bytes::<
            windows::Win32::Devices::Display::DISPLAYCONFIG_MODE_INFO,
        >(&bytes)?);
    }

    Some(RawTopologySnapshot { paths, modes })
}

fn persisted_raw_snapshot_path() -> PathBuf {
    let config_path = monarch::FileConfigStore::default_config_path();
    config_path
        .parent()
        .map(|parent| parent.join("topology_snapshot.json"))
        .unwrap_or_else(|| PathBuf::from("topology_snapshot.json"))
}

fn struct_to_bytes<T>(value: &T) -> Vec<u8> {
    unsafe { std::slice::from_raw_parts((value as *const T).cast::<u8>(), size_of::<T>()).to_vec() }
}

fn struct_from_bytes<T>(bytes: &[u8]) -> Option<T> {
    if bytes.len() != size_of::<T>() {
        return None;
    }

    let mut value = MaybeUninit::<T>::uninit();
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), value.as_mut_ptr().cast::<u8>(), bytes.len());
        Some(value.assume_init())
    }
}
