# Airglow audit roadmap implementation

The user approved implementation of all recommendations in `docs/AUDIT-2026-11-02-DEEP-DIVE.md` and the accompanying summary. This document makes that approved scope concrete. The ten previous bug groups remain fixed; this work adds the engineering, UX and four product MVPs. Decisions below are implementation judgments, not additional user requirements.

## Approach and constraints

Extend the existing Windows Python service and native ES-module UI. Keep the seven main views, twelve themes, current shortcuts, saved layouts, setup and lighting behavior. Add tools within their relevant views, with optional Overview status rather than a new mandatory dashboard. A framework rewrite and four separate applications would introduce migration cost without helping the requested features.

Work in the existing `release/1.11.1` checkout because it contains the user-approved uncommitted audit fixes and other work. Preserve that baseline. No commit, push, release, installed-app patch or real hardware experiment is part of this implementation. Use isolated stores and the simulated rig for validation. No new service, account, paid dependency or remote telemetry. Imports and exports remain local downloads/uploads.

## Dependable control center

Record completed controller cycles, successful sensor samples and completed device writes, separately from queued commands. Use monotonic time for age/duration and epoch timestamps for display/export. Show disconnected, stale, pending, failed and healthy states without claiming a queued write reached hardware. Keep an in-memory, bounded recent-events timeline (200 entries), deduplicate unchanged conditions, record ownership transitions and recovery. Device status must survive reconnect sensibly and must not grow indefinitely with abandoned identities.

Expose status and recent events through `/api/control/*` and compact `state.control` data. Add a Control center tool with actionable links to the relevant existing controls and explicit retry/recheck actions. Add an optional status tile in Overview without changing any existing saved layout. No device is taken over merely by opening this tool.

Strengthen writer retirement/reconnect tests and publish timestamps only on successful work. Controller mutations persist candidates before publishing them; failed saves retain the previous live setting. Preserve failsafe limits, ramp behavior, ownership and control cadence.

## Cooling workbench

An opt-in local recording tool on Fans records temperatures, commanded duty and measured RPM, plus the effective curve and a user workload annotation. Start/stop explicitly; a recorder must never change fan speed. Bound each session to 1,800 samples, sampled no faster than the existing two-second sensor cadence; retain at most 12 saved sessions, with explicit deletion. Preserve missing readings as missing. Persist completed sessions safely, and report save failures without losing the session still in memory.

Show a chronological temperature/duty/RPM chart, select two sessions to compare summaries, and export human-readable JSON and CSV. Compare draft curve targets against a selected recording without applying them. State that workload and ambient conditions affect comparison; make no acoustic/noise claim from RPM. An interrupted recording is identifiable, and all controls work with no sensors/no sessions.

## Profiles and portable rig setup

Saving current settings offers **Update this profile** and **Save as new**. Updating retains the profile ID and automation references. Show included lighting, per-area zone overrides and optional fan curves. A profile that was applied and then adjusted remains available as the update target even though it no longer exactly matches the live settings. Preserve existing include-fans defaults for new saves; make scope explicit. Perform compound persistence under Store locking and keep failed writes from changing memory.

A Settings tool exports a versioned, human-readable rig bundle and imports through validate -> preview -> explicit mapping -> apply. It preserves intended zone names/areas/layout and profiles/curves/automations where valid, remapping references and avoiding ID collisions. Never import device ownership, fan channel/pump control, helper paths, credentials or startup side effects. Preview must expose skipped/unresolved hardware and changed settings. Require an explicit user mapping before changing a different rig; reject duplicate targets and invalid capabilities. Validate everything before committing, protect unread source files, and roll back ordinary write failures. Define the bundle size within the existing 256 KiB request limit. Keep multi-file power-loss limitations explicit.

Factor reusable atomic text/JSON writes (unique same-directory temporary file, flush/fsync, bounded Windows permission retry, cleanup) into one utility used by Store and the effect library. Preserve effect rollback behavior and protected-file checks.

## Studio creator workflow

Always show **Preview only**, **Live draft preview** or **Saved effect applied** using real current controller/trial state. Saving a file is distinct from applying it. Preserve working draft recovery, live-preview cleanup, undo/redo, validation and sandboxing.

Guide the first remix through choose a starting effect -> edit palette/macros/layers -> save -> optionally apply. Existing effect-wide palette/speed/brightness/reactivity controls remain the macro controls; add reusable favorite palettes. Put creative controls first and move author/description/tags/license and exposed-control configuration into appropriate details/publishing areas. Preserve focus and selection across required rerenders, and ensure layer menus and editor controls work at 390px and 700px.

Add portable scene pack export/import with validated effect source, named area assignments and optional control values. Preview the contents/conflicts before installation; never apply a pack automatically. Honor the existing per-effect 32 KB limits, provenance, licenses, sandbox and revocations; no arbitrary file paths or unvalidated source. Reject oversized packs within the API limit. The imported pack must be usable on a simulated second rig without copying hardware IDs.

Split Studio document/normalization/history/persistence and community/publishing responsibilities into focused modules where this reduces coupling. Split API route families along the same domain boundaries, preserving endpoint contracts.

## Shared UX and maintenance

Fan bulk-copy becomes **Copy settings to…** with named target checkboxes, fans selected by default and pumps explicitly opt-in. Show partial failures accurately and retain retry controls. Add a keyboard-accessible Add curve point action; preserve keyboard edits and focus. Compact narrow Fans telemetry so the curve is available earlier.

Automation editing preserves the focused field, caret and keyboard position; ordinary value edits should update the preview without replacing the whole form. Narrow navigation has a visible all-sections control, while keeping direct desktop tabs. Remove redundant decorative eyebrows identified by the audit.

Standardize draft/saving/saved/failed presentation and persistent recovery messages near the relevant surface. Serialize conflicting UI mutations by resource and snapshot request bodies, without blocking unrelated reads or device resources. Never silently retry a non-idempotent action after an uncertain timeout, or replay an obsolete failed edit over a newer successful one.

Consolidate shared spacing/control-size tokens incrementally in the existing CSS. Avoid another broad override layer or theme replacement. Add a Windows CI fixture job for browser/responsive tests, upload artifacts on failure, and cover profile updates, import preview, focus, narrow Studio, failed saves, delayed replies and the new tools. Use a fixture-owned data root and stop its process reliably.

## Completion evidence

Meaningful failing regressions precede behavior changes. Run the entire Python and JS suites, module/import checks, content validators, current visual/responsive suite and new interactive feature tests. Review rendered representative wide and narrow screens and at least contrasting themes. Independently review the final patch against this turn's saved baseline. Update the audit with a coverage ledger for every recommendation, tests, limits and any hardware-only evidence still needed.

Serialless HID identity requires representative physical enumeration before changing the matching algorithm. Keep that known limitation explicit, add diagnostic evidence collection if useful, and do not guess identities. Measure cooling output latency through bounded fake responses and expose measured completion ages; do not claim real thermal qualification.
