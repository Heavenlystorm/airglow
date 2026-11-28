//! Opt-in local cooling session recorder matching `cooling.py`.
//!
//! Records timestamped CPU/GPU/liquid temperature, fan duty, RPM, and per-channel telemetry
//! into `%APPDATA%\Airglow-rs\cooling-sessions.json`, bounded to 1,800 samples per session
//! and 12 retained sessions, with CSV export.

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

pub const MAX_SAMPLES: usize = 1800;
pub const MAX_SESSIONS: usize = 12;
pub const SAMPLE_INTERVAL_SECS: f64 = 2.0;
pub const MAX_ANNOTATION_CHARS: usize = 1000;
pub const COOLING_NOTE: &str =
    "Workload and ambient conditions affect comparisons. RPM is not a noise measurement.";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CoolingChannelSample {
    pub id: String,
    pub name: String,
    pub mode: Option<String>,
    pub rpm: Option<f64>,
    pub target: Option<f64>,
    pub duty: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CoolingCurveMeta {
    pub name: String,
    pub source: String,
    pub failsafe: u8,
    pub points: Vec<[u8; 2]>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CoolingSample {
    pub t: f64,
    pub elapsed: f64,
    pub cpu: Option<f64>,
    pub gpu: Option<f64>,
    pub liquid: Option<f64>,
    pub temp: Option<f64>,
    pub duty: Option<f64>,
    pub rpm: Option<f64>,
    pub curve: Option<CoolingCurveMeta>,
    #[serde(default)]
    pub channels: Vec<CoolingChannelSample>,
}

#[derive(Debug, Clone, Default)]
pub struct CoolingSampleInput {
    pub cpu: Option<f32>,
    pub gpu: Option<f32>,
    pub liquid: Option<f32>,
    pub temp: Option<f32>,
    pub duty: Option<f32>,
    pub rpm: Option<f32>,
    pub curve: Option<CoolingCurveMeta>,
    pub channels: Vec<CoolingChannelSample>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CoolingSession {
    pub id: String,
    pub annotation: String,
    pub started_at: f64,
    pub ended_at: Option<f64>,
    pub ended_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recovery_note: Option<String>,
    #[serde(default)]
    pub samples: Vec<CoolingSample>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CoolingSessionSummary {
    pub id: String,
    pub annotation: String,
    pub started_at: f64,
    pub ended_at: Option<f64>,
    pub ended_reason: Option<String>,
    pub recovery_note: Option<String>,
    pub sample_count: usize,
    pub duration_seconds: f64,
    pub cpu_mean: Option<f64>,
    pub cpu_max: Option<f64>,
    pub gpu_mean: Option<f64>,
    pub gpu_max: Option<f64>,
    pub liquid_mean: Option<f64>,
    pub liquid_max: Option<f64>,
    pub temp_mean: Option<f64>,
    pub temp_max: Option<f64>,
    pub duty_mean: Option<f64>,
    pub duty_max: Option<f64>,
    pub rpm_mean: Option<f64>,
    pub rpm_max: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedCoolingFile {
    version: u32,
    sessions: Vec<CoolingSession>,
    active: Option<CoolingSession>,
}

pub struct CoolingRecorder {
    path: PathBuf,
    sessions: Vec<CoolingSession>,
    active: Option<CoolingSession>,
    started_mono: Option<Instant>,
    last_sample_mono: Option<Instant>,
    load_error: Option<String>,
    save_error: Option<String>,
    seq: u64,
}

impl CoolingRecorder {
    /// Default Airglow-rs data directory (`%APPDATA%\Airglow-rs`).
    pub fn default_dir() -> PathBuf {
        std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."))
            .join("Airglow-rs")
    }

    pub fn new(dir: impl AsRef<Path>) -> Self {
        let path = dir.as_ref().join("cooling-sessions.json");
        let mut rec = Self {
            path,
            sessions: Vec::new(),
            active: None,
            started_mono: None,
            last_sample_mono: None,
            load_error: None,
            save_error: None,
            seq: 0,
        };
        rec.load();
        rec
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn is_recording(&self) -> bool {
        self.active.is_some()
    }

    pub fn active_session(&self) -> Option<&CoolingSession> {
        self.active.as_ref()
    }

    pub fn active_sample_count(&self) -> usize {
        self.active.as_ref().map_or(0, |s| s.samples.len())
    }

    pub fn sessions(&self) -> &[CoolingSession] {
        &self.sessions
    }

    pub fn load_error(&self) -> Option<&str> {
        self.load_error.as_deref()
    }

    pub fn save_error(&self) -> Option<&str> {
        self.save_error.as_deref()
    }

    /// Returns summaries of completed sessions, newest first.
    pub fn summaries(&self) -> Vec<CoolingSessionSummary> {
        self.sessions
            .iter()
            .rev()
            .map(Self::summarize_session)
            .collect()
    }

    pub fn active_summary(&self) -> Option<CoolingSessionSummary> {
        self.active.as_ref().map(Self::summarize_session)
    }

    pub fn summarize_session(session: &CoolingSession) -> CoolingSessionSummary {
        let samples = &session.samples;
        let duration_seconds = samples.last().map_or(0.0, |s| s.elapsed);
        let stats = |f: fn(&CoolingSample) -> Option<f64>| -> (Option<f64>, Option<f64>) {
            let mut sum = 0.0;
            let mut count = 0usize;
            let mut max_v = f64::NEG_INFINITY;
            for s in samples {
                if let Some(v) = f(s)
                    && v.is_finite()
                {
                    sum += v;
                    count += 1;
                    if v > max_v {
                        max_v = v;
                    }
                }
            }
            if count == 0 {
                (None, None)
            } else {
                (
                    Some(round2(sum / count as f64)),
                    Some(round3(max_v)),
                )
            }
        };

        let (cpu_mean, cpu_max) = stats(|s| s.cpu);
        let (gpu_mean, gpu_max) = stats(|s| s.gpu);
        let (liquid_mean, liquid_max) = stats(|s| s.liquid);
        let (temp_mean, temp_max) = stats(|s| s.temp);
        let (duty_mean, duty_max) = stats(|s| s.duty);
        let (rpm_mean, rpm_max) = stats(|s| s.rpm);

        CoolingSessionSummary {
            id: session.id.clone(),
            annotation: session.annotation.clone(),
            started_at: session.started_at,
            ended_at: session.ended_at,
            ended_reason: session.ended_reason.clone(),
            recovery_note: session.recovery_note.clone(),
            sample_count: samples.len(),
            duration_seconds,
            cpu_mean,
            cpu_max,
            gpu_mean,
            gpu_max,
            liquid_mean,
            liquid_max,
            temp_mean,
            temp_max,
            duty_mean,
            duty_max,
            rpm_mean,
            rpm_max,
        }
    }

    /// Starts a new opt-in cooling recording session. Returns the new session ID.
    pub fn start(&mut self, annotation: impl Into<String>) -> Result<String> {
        if let Some(err) = &self.load_error {
            bail!("{err}");
        }
        if self.active.is_some() {
            bail!("A cooling recording is already running");
        }
        if self.sessions.len() >= MAX_SESSIONS {
            bail!("12 recordings are retained; export and delete one before starting another");
        }
        let annotation = annotation.into();
        if annotation.chars().count() > MAX_ANNOTATION_CHARS {
            bail!("Workload annotation must be at most 1,000 characters");
        }

        let id = self.next_session_id();
        let session = CoolingSession {
            id: id.clone(),
            annotation: annotation.trim().to_string(),
            started_at: round3(now_epoch_secs()),
            ended_at: None,
            ended_reason: None,
            recovery_note: None,
            samples: Vec::new(),
        };

        self.active = Some(session);
        self.started_mono = Some(Instant::now());
        self.last_sample_mono = None;

        if let Err(e) = self.persist() {
            self.active = None;
            self.started_mono = None;
            return Err(e);
        }
        Ok(id)
    }

    /// Appends a sample if a recording is active and at least `SAMPLE_INTERVAL_SECS` (2 s)
    /// has passed since the last sample. Returns `true` if a sample was recorded.
    pub fn record_sample(&mut self, input: CoolingSampleInput) -> bool {
        if self.active.is_none() {
            return false;
        }
        let now = Instant::now();
        if let Some(last) = self.last_sample_mono
            && now.duration_since(last).as_secs_f64() < SAMPLE_INTERVAL_SECS
        {
            return false;
        }
        let elapsed = self
            .started_mono
            .map_or(0.0, |s| now.duration_since(s).as_secs_f64());
        self.last_sample_mono = Some(now);
        self.record_sample_at(input, elapsed, now_epoch_secs())
    }

    /// Appends a sample with explicit `elapsed_s` and `epoch_s` timestamps (useful for tests).
    /// Automatically finishes and saves the session upon reaching `MAX_SAMPLES` (1,800).
    pub fn record_sample_at(
        &mut self,
        input: CoolingSampleInput,
        elapsed_s: f64,
        epoch_s: f64,
    ) -> bool {
        let Some(active) = self.active.as_mut() else {
            return false;
        };

        let sample = CoolingSample {
            t: round3(epoch_s),
            elapsed: round3(elapsed_s.max(0.0)),
            cpu: finite_f32(input.cpu),
            gpu: finite_f32(input.gpu),
            liquid: finite_f32(input.liquid),
            temp: finite_f32(input.temp),
            duty: finite_f32(input.duty),
            rpm: finite_f32(input.rpm),
            curve: input.curve,
            channels: input.channels.into_iter().take(64).collect(),
        };
        active.samples.push(sample);

        if active.samples.len() >= MAX_SAMPLES {
            let _ = self.finish_active("sample_limit");
        }
        true
    }

    /// Stops the active recording, moves it to completed sessions, and persists to disk.
    pub fn stop(&mut self) -> Result<CoolingSession> {
        self.finish_active("stopped")
    }

    /// Deletes a completed session by ID and persists the updated history.
    pub fn delete_session(&mut self, session_id: &str) -> Result<()> {
        if self.active.as_ref().is_some_and(|s| s.id == session_id) {
            bail!("Stop recording before deleting it");
        }
        let before = self.sessions.len();
        self.sessions.retain(|s| s.id != session_id);
        if self.sessions.len() == before {
            bail!("Recording not found");
        }
        self.persist()
    }

    /// Looks up either a completed session or the active recording by ID.
    pub fn get(&self, session_id: &str) -> Option<&CoolingSession> {
        self.sessions
            .iter()
            .find(|s| s.id == session_id)
            .or_else(|| self.active.as_ref().filter(|s| s.id == session_id))
    }

    /// Exports a recorded session as CSV matching Python Airglow's column layout and formula safety.
    pub fn export_csv(&self, session_id: &str) -> Result<String> {
        let session = self.get(session_id).context("Recording not found")?;
        let mut out = String::from(
            "time_epoch_s,elapsed_s,cpu_c,gpu_c,liquid_c,source_c,curve_commanded_duty_pct,measured_rpm,curve,temperature_source,channel_id,channel_name,channel_mode,channel_target_pct,channel_measured_duty_pct,channel_rpm,workload\r\n",
        );

        let empty_channels = [CoolingChannelSample {
            id: String::new(),
            name: String::new(),
            mode: None,
            rpm: None,
            target: None,
            duty: None,
        }];

        for sample in &session.samples {
            let curve_name = sample
                .curve
                .as_ref()
                .map(|c| c.name.as_str())
                .unwrap_or("");
            let curve_src = sample
                .curve
                .as_ref()
                .map(|c| c.source.as_str())
                .unwrap_or("");
            let channels: &[CoolingChannelSample] = if sample.channels.is_empty() {
                &empty_channels
            } else {
                &sample.channels
            };

            for ch in channels {
                let cols = [
                    fmt_f64(Some(sample.t)),
                    fmt_f64(Some(sample.elapsed)),
                    fmt_f64(sample.cpu),
                    fmt_f64(sample.gpu),
                    fmt_f64(sample.liquid),
                    fmt_f64(sample.temp),
                    fmt_f64(sample.duty),
                    fmt_f64(sample.rpm),
                    csv_cell(curve_name),
                    csv_cell(curve_src),
                    csv_cell(&ch.id),
                    csv_cell(&ch.name),
                    csv_cell(ch.mode.as_deref().unwrap_or("")),
                    fmt_f64(ch.target),
                    fmt_f64(ch.duty),
                    fmt_f64(ch.rpm),
                    csv_cell(&session.annotation),
                ];
                out.push_str(&cols.join(","));
                out.push_str("\r\n");
            }
        }
        Ok(out)
    }

    /// Exports a session's CSV directly to `dest_dir/airglow-cooling-<id12>.csv`.
    pub fn export_csv_to_file(&self, session_id: &str, dest_dir: &Path) -> Result<PathBuf> {
        let csv = self.export_csv(session_id)?;
        fs::create_dir_all(dest_dir).context("create cooling export directory")?;
        let short_id: String = session_id.chars().take(12).collect();
        let file = dest_dir.join(format!("airglow-cooling-{short_id}.csv"));
        fs::write(&file, csv).with_context(|| format!("write {}", file.display()))?;
        Ok(file)
    }

    fn finish_active(&mut self, reason: &str) -> Result<CoolingSession> {
        let mut session = self
            .active
            .take()
            .context("No cooling recording is running")?;
        self.started_mono = None;
        self.last_sample_mono = None;
        session.ended_at = Some(round3(now_epoch_secs()));
        session.ended_reason = Some(reason.to_string());
        let cloned = session.clone();
        self.sessions.push(session);
        if let Err(e) = self.persist() {
            self.save_error = Some(format!("Recording is kept in memory; save failed: {e}"));
        } else {
            self.save_error = None;
        }
        Ok(cloned)
    }

    fn load(&mut self) {
        if !self.path.is_file() {
            return;
        }
        let result: Result<()> = (|| {
            let raw = fs::read_to_string(&self.path).context("read cooling-sessions.json")?;
            let mut file: PersistedCoolingFile =
                serde_json::from_str(&raw).context("parse cooling-sessions.json")?;
            if file.version != 1 {
                bail!("unknown recording format version {}", file.version);
            }
            if file.sessions.len() + usize::from(file.active.is_some()) > MAX_SESSIONS {
                bail!("invalid session count");
            }
            for s in &file.sessions {
                if s.samples.len() > MAX_SAMPLES {
                    bail!("session exceeds sample limit");
                }
            }
            if let Some(mut interrupted) = file.active.take() {
                if interrupted.samples.len() > MAX_SAMPLES {
                    interrupted.samples.truncate(MAX_SAMPLES);
                }
                interrupted.ended_at = None;
                interrupted.ended_reason = Some("interrupted".to_string());
                interrupted.recovery_note = Some(if interrupted.samples.is_empty() {
                    "Airglow stopped before this recording was saved. Only the start marker survived."
                        .to_string()
                } else {
                    format!(
                        "Airglow stopped while recording. {} saved samples were recovered; later samples may be missing.",
                        interrupted.samples.len()
                    )
                });
                file.sessions.push(interrupted);
            }
            self.sessions = file.sessions;
            Ok(())
        })();

        if let Err(e) = result {
            self.load_error = Some(format!(
                "Cooling history could not be read: {e}. The original file is protected."
            ));
        }
    }

    fn persist(&mut self) -> Result<()> {
        if let Some(err) = &self.load_error {
            bail!("{err}");
        }
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).context("create cooling data directory")?;
        }
        let doc = PersistedCoolingFile {
            version: 1,
            sessions: self.sessions.clone(),
            active: self.active.clone(),
        };
        let text = serde_json::to_string_pretty(&doc)?;
        let tmp = self.path.with_extension("json.tmp");
        fs::write(&tmp, text).context("write temporary cooling sessions file")?;
        fs::rename(&tmp, &self.path).context("replace cooling-sessions.json")?;
        self.save_error = None;
        Ok(())
    }

    fn next_session_id(&mut self) -> String {
        self.seq = self.seq.wrapping_add(1);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        format!("{:016x}{:08x}{:08x}", nanos as u64, std::process::id(), self.seq as u32)
    }
}

fn now_epoch_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

fn round3(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

fn finite_f32(v: Option<f32>) -> Option<f64> {
    v.filter(|x| x.is_finite()).map(|x| round3(x as f64))
}

fn fmt_f64(v: Option<f64>) -> String {
    match v {
        Some(n) if n.is_finite() => format!("{n}"),
        _ => String::new(),
    }
}

fn csv_cell(raw: &str) -> String {
    let trimmed = raw.trim_start();
    let needs_prefix = trimmed.starts_with(['=', '+', '-', '@']);
    let safe = if needs_prefix {
        format!("'{raw}")
    } else {
        raw.to_string()
    };
    if safe.contains([',', '"', '\r', '\n']) {
        format!("\"{}\"", safe.replace('"', "\"\""))
    } else {
        safe
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "airglow_cooling_test_{}_{}",
            tag,
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn start_record_stop_persist_and_export_csv() {
        let dir = temp_dir("basic");
        let mut rec = CoolingRecorder::new(&dir);
        assert!(!rec.is_recording());

        let id = rec.start("=Cinebench R24, 10m").expect("start");
        assert!(rec.is_recording());

        assert!(rec.record_sample_at(
            CoolingSampleInput {
                cpu: Some(68.5),
                gpu: Some(52.0),
                liquid: Some(34.2),
                temp: Some(68.5),
                duty: Some(60.0),
                rpm: Some(1250.0),
                curve: Some(CoolingCurveMeta {
                    name: "balanced".to_string(),
                    source: "max".to_string(),
                    failsafe: 30,
                    points: vec![[40, 30], [88, 100]],
                }),
                channels: vec![CoolingChannelSample {
                    id: "/lpc/nct6701d/0/control/0".to_string(),
                    name: "Front Fans".to_string(),
                    mode: Some("curve".to_string()),
                    rpm: Some(1250.0),
                    target: Some(60.0),
                    duty: Some(60.0),
                }],
            },
            0.0,
            1_700_000_000.0
        ));
        assert!(rec.record_sample_at(
            CoolingSampleInput {
                cpu: Some(72.5),
                gpu: Some(54.0),
                temp: Some(72.5),
                duty: Some(70.0),
                ..CoolingSampleInput::default()
            },
            2.0,
            1_700_000_002.0
        ));

        let finished = rec.stop().expect("stop");
        assert_eq!(finished.samples.len(), 2);
        assert_eq!(finished.ended_reason.as_deref(), Some("stopped"));

        // Reload from disk
        let reloaded = CoolingRecorder::new(&dir);
        assert_eq!(reloaded.sessions().len(), 1);
        let summary = &reloaded.summaries()[0];
        assert_eq!(summary.id, id);
        assert_eq!(summary.sample_count, 2);
        assert_eq!(summary.cpu_mean, Some(70.5));
        assert_eq!(summary.cpu_max, Some(72.5));

        let csv = reloaded.export_csv(&id).expect("csv");
        assert!(csv.starts_with("time_epoch_s,elapsed_s,cpu_c,"));
        assert!(csv.contains("\"'=Cinebench R24, 10m\""));
        assert!(csv.contains("/lpc/nct6701d/0/control/0"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn enforces_sample_limit_and_max_sessions() {
        let dir = temp_dir("bounds");
        let mut rec = CoolingRecorder::new(&dir);

        rec.start("long run").unwrap();
        for i in 0..MAX_SAMPLES {
            rec.record_sample_at(
                CoolingSampleInput {
                    cpu: Some(55.0),
                    ..CoolingSampleInput::default()
                },
                (i as f64) * 2.0,
                1_700_000_000.0 + (i as f64) * 2.0,
            );
        }
        assert!(!rec.is_recording());
        assert_eq!(rec.sessions().len(), 1);
        assert_eq!(rec.sessions()[0].samples.len(), MAX_SAMPLES);
        assert_eq!(
            rec.sessions()[0].ended_reason.as_deref(),
            Some("sample_limit")
        );

        for n in 1..MAX_SESSIONS {
            rec.start(format!("run {n}")).unwrap();
            rec.stop().unwrap();
        }
        assert_eq!(rec.sessions().len(), MAX_SESSIONS);
        assert!(rec.start("overflow").is_err());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn recovers_interrupted_recording_on_reload() {
        let dir = temp_dir("recover");
        {
            let mut rec = CoolingRecorder::new(&dir);
            rec.start("crashed mid-run").unwrap();
            assert!(rec.is_recording());
        }
        let reloaded = CoolingRecorder::new(&dir);
        assert!(!reloaded.is_recording());
        assert_eq!(reloaded.sessions().len(), 1);
        assert_eq!(
            reloaded.sessions()[0].ended_reason.as_deref(),
            Some("interrupted")
        );
        assert!(reloaded.sessions()[0].recovery_note.is_some());

        let _ = fs::remove_dir_all(&dir);
    }
}
