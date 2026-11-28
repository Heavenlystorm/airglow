//! Zero-dependency (Win32 FFI + `std`) real-time lighting input analyzers:
//! - [`ScreenSampler`]: captures a downsampled 48x27 RGB desktop frame via Win32 GDI.
//! - [`AudioAnalyzer`]: captures system audio via Windows WASAPI loopback COM + pure-Rust
//!   radix-2 FFT into 24 log-spaced spectrum bands and bass/beat energy, plus a 120 BPM
//!   synthetic `test_beat` mode for Studio previewing.
//! - [`InputContext`]: lightweight per-frame snapshot passed into `effects::render_zone`.

use std::f32::consts::PI;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// Horizontal resolution of the downsampled screen ambience grid.
pub const SCREEN_W: usize = 48;
/// Vertical resolution of the downsampled screen ambience grid.
pub const SCREEN_H: usize = 27;
/// Number of log-spaced frequency bands in [`AudioFrame`].
pub const AUDIO_BANDS: usize = 24;
/// Radix-2 FFT block size for audio spectrum analysis.
pub const FFT_SIZE: usize = 1024;

const SCREEN_MIN_INTERVAL: Duration = Duration::from_millis(50); // ~20 FPS
const SCREEN_SMOOTH: f32 = 0.45;
const AUDIO_SILENCE_DECAY_AFTER: Duration = Duration::from_millis(80);
const DEFAULT_SAMPLE_RATE: u32 = 48_000;

// -----------------------------------------------------------------------------
// Screen capture & sampling
// -----------------------------------------------------------------------------

/// Immutable, cheaply cloneable RGB float frame (`0.0..=1.0`) captured from the desktop.
#[derive(Debug, Clone)]
pub struct ScreenFrame {
    width: usize,
    height: usize,
    pixels: Arc<Vec<[f32; 3]>>,
}

impl ScreenFrame {
    /// Construct a [`ScreenFrame`] from a row-major RGB float grid (`width * height` pixels).
    /// Values are clamped to `0.0..=1.0`. If `pixels.len() < width * height`, missing entries
    /// are padded with black.
    pub fn from_grid(width: usize, height: usize, mut pixels: Vec<[f32; 3]>) -> Self {
        let expected = width.saturating_mul(height);
        for px in &mut pixels {
            px[0] = clamp01(px[0]);
            px[1] = clamp01(px[1]);
            px[2] = clamp01(px[2]);
        }
        if pixels.len() < expected {
            pixels.resize(expected, [0.0, 0.0, 0.0]);
        } else if pixels.len() > expected {
            pixels.truncate(expected);
        }
        Self {
            width,
            height,
            pixels: Arc::new(pixels),
        }
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn height(&self) -> usize {
        self.height
    }

    pub fn pixels(&self) -> &[[f32; 3]] {
        &self.pixels
    }

    /// Sample the screen colour at normalized coordinates `norm_pos = [x, y]` (`0.0..=1.0`).
    /// When `mirror` is true, `x` is flipped horizontally (`1.0 - x`).
    /// Out-of-range or non-finite coordinates are safely clamped.
    pub fn sample(&self, norm_pos: [f32; 2], mirror: bool) -> [f32; 3] {
        if self.width == 0 || self.height == 0 || self.pixels.is_empty() {
            return [0.0, 0.0, 0.0];
        }
        let mut x = clamp01(norm_pos[0]);
        let y = clamp01(norm_pos[1]);
        if mirror {
            x = 1.0 - x;
        }
        let xi = ((x * (self.width - 1) as f32).round() as usize).min(self.width - 1);
        let yi = ((y * (self.height - 1) as f32).round() as usize).min(self.height - 1);
        self.pixels[yi * self.width + xi]
    }

    /// Sample the screen colour and apply Airglow's saturation colour boost (`saturation: 0..=100`).
    pub fn sample_boosted(&self, norm_pos: [f32; 2], mirror: bool, saturation: u8) -> [f32; 3] {
        let rgb = self.sample(norm_pos, mirror);
        boost_saturation(rgb, saturation)
    }
}

/// Applies Airglow's saturation boost (`boost = 1.0 + saturation / 50.0`) clamped to `0.0..=1.0`.
#[inline]
pub fn boost_saturation(rgb: [f32; 3], saturation: u8) -> [f32; 3] {
    let boost = 1.0 + (saturation.min(100) as f32) / 50.0;
    let grey = (rgb[0] + rgb[1] + rgb[2]) / 3.0;
    [
        (grey + (rgb[0] - grey) * boost).clamp(0.0, 1.0),
        (grey + (rgb[1] - grey) * boost).clamp(0.0, 1.0),
        (grey + (rgb[2] - grey) * boost).clamp(0.0, 1.0),
    ]
}

/// Captures a smoothed 48x27 RGB desktop grid on Windows via Win32 GDI (`StretchBlt` + `GetDIBits`),
/// with a safe empty fallback on non-Windows platforms or when capture is unavailable.
///
/// Only captures while [`ScreenSampler::is_active`] is `true` and at most ~20 times per second.
#[derive(Debug)]
pub struct ScreenSampler {
    active: bool,
    display: u8,
    frame: Option<ScreenFrame>,
    smooth_buf: Vec<[f32; 3]>,
    last_capture: Option<Instant>,
    last_error: Option<String>,
}

impl Default for ScreenSampler {
    fn default() -> Self {
        Self::new()
    }
}

impl ScreenSampler {
    pub fn new() -> Self {
        Self {
            active: false,
            display: 1,
            frame: None,
            smooth_buf: Vec::new(),
            last_capture: None,
            last_error: None,
        }
    }

    /// Construct a sampler pre-populated with a deterministic [`ScreenFrame`] (useful for tests/previews).
    pub fn from_frame(frame: ScreenFrame) -> Self {
        let smooth_buf = frame.pixels.as_ref().clone();
        Self {
            active: true,
            display: 1,
            frame: Some(frame),
            smooth_buf,
            last_capture: Some(Instant::now()),
            last_error: None,
        }
    }

    /// Enable or disable screen capture. Disabling clears the cached frame so stale images are dropped.
    pub fn set_active(&mut self, active: bool) {
        if self.active == active {
            return;
        }
        self.active = active;
        if !active {
            self.frame = None;
            self.smooth_buf.clear();
            self.last_capture = None;
        }
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Select which display to capture: `0` = all displays (virtual desktop), `1` = primary display,
    /// `2+` = additional displays (falls back to primary if out of range).
    pub fn set_display(&mut self, display: u8) {
        if self.display != display {
            self.display = display;
            self.frame = None;
            self.smooth_buf.clear();
            self.last_capture = None;
        }
    }

    pub fn display(&self) -> u8 {
        self.display
    }

    pub fn has_frame(&self) -> bool {
        self.frame.is_some()
    }

    pub fn frame(&self) -> Option<ScreenFrame> {
        self.frame.clone()
    }

    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    /// Sample the most recently captured frame at normalized coordinates `norm_pos = [x, y]` (`0.0..=1.0`).
    /// Returns `[0.0, 0.0, 0.0]` if no frame has been captured yet.
    pub fn sample(&self, norm_pos: [f32; 2], mirror: bool) -> [f32; 3] {
        match &self.frame {
            Some(f) => f.sample(norm_pos, mirror),
            None => [0.0, 0.0, 0.0],
        }
    }

    /// Capture a new desktop frame if `active` and at least 50 ms (~20 FPS) has elapsed since the
    /// previous capture. Returns `true` if a new frame was captured.
    pub fn refresh_if_due(&mut self) -> bool {
        if !self.active {
            return false;
        }
        if let Some(last) = self.last_capture
            && last.elapsed() < SCREEN_MIN_INTERVAL
        {
            return false;
        }
        self.refresh_now()
    }

    /// Immediately attempt a desktop capture when `active`. Returns `true` on success.
    pub fn refresh_now(&mut self) -> bool {
        if !self.active {
            return false;
        }
        self.last_capture = Some(Instant::now());
        match capture_screen_grid(self.display, SCREEN_W, SCREEN_H) {
            Ok(raw) => {
                self.last_error = None;
                self.ingest_raw_grid(SCREEN_W, SCREEN_H, &raw);
                true
            }
            Err(err) => {
                self.last_error = Some(err);
                false
            }
        }
    }

    fn ingest_raw_grid(&mut self, width: usize, height: usize, raw: &[[f32; 3]]) {
        let len = width * height;
        if raw.len() != len {
            return;
        }
        if self.smooth_buf.len() != len {
            self.smooth_buf = raw.to_vec();
        } else {
            for (dst, src) in self.smooth_buf.iter_mut().zip(raw.iter()) {
                dst[0] += (src[0] - dst[0]) * SCREEN_SMOOTH;
                dst[1] += (src[1] - dst[1]) * SCREEN_SMOOTH;
                dst[2] += (src[2] - dst[2]) * SCREEN_SMOOTH;
            }
        }
        self.frame = Some(ScreenFrame::from_grid(width, height, self.smooth_buf.clone()));
    }
}

// -----------------------------------------------------------------------------
// Audio spectrum & beat analysis
// -----------------------------------------------------------------------------

/// Normalized real-time audio analysis snapshot passed to reactive lighting effects.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AudioFrame {
    /// 24 log-spaced frequency bands from bass (`bands[0]`) to treble (`bands[23]`), each `0.0..=1.0`.
    pub bands: [f32; AUDIO_BANDS],
    /// Beat/low-end energy (`0.0..=1.0`).
    pub bass: f32,
    /// Mean spectrum level (`0.0..=1.0`).
    pub level: f32,
    /// True on a detected beat transient.
    pub beat: bool,
}

impl Default for AudioFrame {
    fn default() -> Self {
        Self {
            bands: [0.0; AUDIO_BANDS],
            bass: 0.0,
            level: 0.0,
            beat: false,
        }
    }
}

impl AudioFrame {
    /// Sample the 24-band spectrum at normalized position `norm_x` (`0.0` = bass, `1.0` = treble).
    /// Values >= `1.0` stay on the highest treble band rather than wrapping back to bass.
    pub fn band_at(&self, norm_x: f32) -> f32 {
        if !norm_x.is_finite() {
            return 0.0;
        }
        let x = if norm_x >= 1.0 {
            1.0 - 1e-6
        } else {
            norm_x.rem_euclid(1.0)
        };
        let idx = ((x * AUDIO_BANDS as f32).floor() as usize).min(AUDIO_BANDS - 1);
        self.bands[idx]
    }
}

/// Detects beats when bass energy jumps well above its recent average, rate-limited by `gap_secs`.
#[derive(Debug, Clone)]
pub struct BeatDetector {
    ratio: f32,
    floor: f32,
    gap_secs: f32,
    avg: f32,
    last_beat_secs: f32,
}

impl Default for BeatDetector {
    fn default() -> Self {
        Self::new()
    }
}

impl BeatDetector {
    pub fn new() -> Self {
        Self {
            ratio: 1.45,
            floor: 0.25,
            gap_secs: 0.25,
            avg: 0.0,
            last_beat_secs: -1.0,
        }
    }

    pub fn update(&mut self, bass: f32, now_secs: f32) -> bool {
        let bass = clamp01(bass);
        let is_beat = bass > self.floor
            && bass > self.avg * self.ratio
            && (now_secs - self.last_beat_secs) >= self.gap_secs;
        if is_beat {
            self.last_beat_secs = now_secs;
        }
        self.avg = self.avg * 0.9 + bass * 0.1;
        is_beat
    }

    pub fn reset(&mut self) {
        self.avg = 0.0;
        self.last_beat_secs = -1.0;
    }
}

/// Pure-Rust Hann-windowed radix-2 FFT and 24-band log-spaced spectrum analyzer with slow
/// auto-gain and fast-attack / soft-release smoothing.
#[derive(Debug, Clone)]
pub struct SpectrumProcessor {
    sample_rate: u32,
    window: Vec<f32>,
    band_bins: Vec<(usize, usize)>,
    peak: [f32; AUDIO_BANDS],
    smooth: [f32; AUDIO_BANDS],
    bass: f32,
    beats: BeatDetector,
}

impl SpectrumProcessor {
    pub fn new(sample_rate: u32) -> Self {
        let rate = sample_rate.max(8_000);
        let window: Vec<f32> = (0..FFT_SIZE)
            .map(|i| 0.5 * (1.0 - (2.0 * PI * i as f32 / (FFT_SIZE - 1) as f32).cos()))
            .collect();
        let band_bins = build_log_band_bins(rate, FFT_SIZE, AUDIO_BANDS, 40.0, 16_000.0);
        Self {
            sample_rate: rate,
            window,
            band_bins,
            peak: [1e-3; AUDIO_BANDS],
            smooth: [0.0; AUDIO_BANDS],
            bass: 0.0,
            beats: BeatDetector::new(),
        }
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn reset(&mut self) {
        self.peak = [1e-3; AUDIO_BANDS];
        self.smooth = [0.0; AUDIO_BANDS];
        self.bass = 0.0;
        self.beats.reset();
    }

    /// Analyze a block of mono PCM samples (`-1.0..=1.0`, zero-padded if shorter than [`FFT_SIZE`])
    /// and return the updated [`AudioFrame`].
    pub fn process_mono(&mut self, samples: &[f32], now_secs: f32) -> AudioFrame {
        let mut re = [0.0f32; FFT_SIZE];
        let mut im = [0.0f32; FFT_SIZE];
        let take = samples.len().min(FFT_SIZE);
        for i in 0..take {
            let s = if samples[i].is_finite() {
                samples[i]
            } else {
                0.0
            };
            re[i] = s * self.window[i];
        }

        fft_radix2(&mut re, &mut im);

        let half = FFT_SIZE / 2 + 1;
        let inv_scale = 2.0 / FFT_SIZE as f32;
        let mut mags = [0.0f32; FFT_SIZE / 2 + 1];
        for k in 0..half {
            mags[k] = (re[k] * re[k] + im[k] * im[k]).sqrt() * inv_scale;
        }

        let mut norm = [0.0f32; AUDIO_BANDS];
        for (b, &(lo_bin, hi_bin)) in self.band_bins.iter().enumerate() {
            let count = (hi_bin - lo_bin + 1).max(1) as f32;
            let mut sum = 0.0f32;
            for mag in &mags[lo_bin..=hi_bin] {
                sum += *mag;
            }
            let raw = sum / count;
            self.peak[b] = raw.max(self.peak[b] * 0.995);
            let n = (raw / self.peak[b].max(1e-4)).clamp(0.0, 1.0);
            norm[b] = n;
            self.smooth[b] = if n > self.smooth[b] {
                n
            } else {
                self.smooth[b] * 0.82
            };
        }

        let raw_bass = (norm[0] + norm[1] + norm[2] + norm[3]) * 0.25;
        self.bass = if raw_bass > self.bass {
            raw_bass
        } else {
            self.bass * 0.80
        };
        let beat = self.beats.update(raw_bass, now_secs);
        let level = self.smooth.iter().sum::<f32>() / AUDIO_BANDS as f32;

        AudioFrame {
            bands: self.smooth,
            bass: clamp01(self.bass),
            level: clamp01(level),
            beat,
        }
    }

    /// Smoothly decay bands and bass toward silence when no audio packets arrive.
    pub fn decay(&mut self) -> AudioFrame {
        for s in &mut self.smooth {
            *s *= 0.82;
            if *s < 1e-4 {
                *s = 0.0;
            }
        }
        self.bass *= 0.80;
        if self.bass < 1e-4 {
            self.bass = 0.0;
        }
        let level = self.smooth.iter().sum::<f32>() / AUDIO_BANDS as f32;
        AudioFrame {
            bands: self.smooth,
            bass: clamp01(self.bass),
            level: clamp01(level),
            beat: false,
        }
    }
}

/// Generates a crisp rhythmic 120 BPM beat + 24-band spectrum curve for Studio previewing ("Test beat").
pub fn synthetic_test_beat(t: f32) -> AudioFrame {
    let t = if t.is_finite() { t } else { 0.0 };
    // 120 BPM = 2 beats per second (0.5 s period).
    let beat_phase = (t * 2.0).rem_euclid(1.0);
    let offbeat_phase = (t * 2.0 + 0.5).rem_euclid(1.0);
    let bar_phase = (t * 0.5).rem_euclid(1.0); // 4-beat bar (2.0 s)

    let kick = (-beat_phase * 7.5).exp();
    let hat = (-offbeat_phase * 12.0).exp();
    let snare_window = ((t * 1.0 + 0.5).rem_euclid(1.0)) * 2.0; // hits on beats 2 & 4
    let snare = if snare_window < 1.0 {
        (-snare_window * 9.0).exp()
    } else {
        0.0
    };

    let mut bands = [0.0f32; AUDIO_BANDS];
    for (i, slot) in bands.iter_mut().enumerate() {
        let u = i as f32 / (AUDIO_BANDS - 1) as f32;
        let val = if i < 6 {
            // Sub-bass & kick punch
            let bell = 1.0 - (i as f32 / 6.0) * 0.35;
            kick * bell
        } else if i < 16 {
            // Mid-range melodic groove + snare body
            let melody = 0.5 + 0.5 * ((u * 8.0 - bar_phase * 2.0) * PI).sin();
            0.14 + kick * 0.35 + snare * 0.42 + melody * 0.28 * (0.4 + 0.6 * kick)
        } else {
            // High-frequency hi-hat & air shimmer
            let shimmer = 0.5 + 0.5 * ((u * 14.0 + t * 6.0) * PI).cos();
            hat * 0.72 + snare * 0.25 + kick * 0.15 + shimmer * 0.10
        };
        *slot = val.clamp(0.0, 1.0);
    }

    let bass = kick.clamp(0.0, 1.0);
    let level = (bands.iter().sum::<f32>() / AUDIO_BANDS as f32).clamp(0.0, 1.0);
    let beat = beat_phase < 0.12;

    AudioFrame {
        bands,
        bass,
        level,
        beat,
    }
}

/// Real-time system audio analyzer backed by Windows WASAPI loopback capture on a lazy worker
/// thread, plus a deterministic 120 BPM `test_beat` preview mode.
pub struct AudioAnalyzer {
    active: bool,
    test_beat: bool,
    started: Instant,
    shared: Arc<Mutex<AudioSharedState>>,
    stop_flag: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

#[derive(Debug)]
struct AudioSharedState {
    frame: AudioFrame,
    last_packet: Option<Instant>,
    capturing: bool,
    error: Option<String>,
}

impl Default for AudioSharedState {
    fn default() -> Self {
        Self {
            frame: AudioFrame::default(),
            last_packet: None,
            capturing: false,
            error: None,
        }
    }
}

impl Default for AudioAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for AudioAnalyzer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AudioAnalyzer")
            .field("active", &self.active)
            .field("test_beat", &self.test_beat)
            .field("capturing", &self.is_capturing())
            .finish()
    }
}

impl AudioAnalyzer {
    pub fn new() -> Self {
        Self {
            active: false,
            test_beat: false,
            started: Instant::now(),
            shared: Arc::new(Mutex::new(AudioSharedState::default())),
            stop_flag: Arc::new(AtomicBool::new(false)),
            worker: None,
        }
    }

    /// Enable or disable live OS audio capture. When `active` is `true` and `test_beat` is `false`,
    /// the background WASAPI loopback thread is started lazily.
    pub fn set_active(&mut self, active: bool) {
        if self.active == active {
            return;
        }
        self.active = active;
        self.sync_worker();
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Toggle synthetic 120 BPM Studio preview beat mode.
    pub fn set_test_beat(&mut self, enabled: bool) {
        if self.test_beat == enabled {
            return;
        }
        self.test_beat = enabled;
        self.sync_worker();
    }

    pub fn is_test_beat(&self) -> bool {
        self.test_beat
    }

    /// True while the background audio capture thread is actively streaming from an OS endpoint.
    pub fn is_capturing(&self) -> bool {
        self.shared.lock().map(|g| g.capturing).unwrap_or(false)
    }

    pub fn last_error(&self) -> Option<String> {
        self.shared.lock().ok().and_then(|g| g.error.clone())
    }

    /// Return the current [`AudioFrame`] for animation time `t` (seconds).
    /// - If `test_beat` is enabled, returns [`synthetic_test_beat(t)`].
    /// - Otherwise if `active`, returns the latest smoothed live loopback frame.
    /// - Otherwise returns silence ([`AudioFrame::default()`]).
    pub fn frame(&self, t: f32) -> AudioFrame {
        if self.test_beat {
            return synthetic_test_beat(t);
        }
        if !self.active {
            return AudioFrame::default();
        }
        if let Ok(mut state) = self.shared.lock() {
            if let Some(last) = state.last_packet
                && last.elapsed() > AUDIO_SILENCE_DECAY_AFTER
            {
                // Decay gracefully when WASAPI loopback delivers no packets (silence/paused).
                for b in &mut state.frame.bands {
                    *b *= 0.85;
                    if *b < 1e-4 {
                        *b = 0.0;
                    }
                }
                state.frame.bass *= 0.82;
                if state.frame.bass < 1e-4 {
                    state.frame.bass = 0.0;
                }
                state.frame.level =
                    (state.frame.bands.iter().sum::<f32>() / AUDIO_BANDS as f32).clamp(0.0, 1.0);
                state.frame.beat = false;
            }
            return state.frame;
        }
        AudioFrame::default()
    }

    /// Convenience wrapper around [`AudioAnalyzer::frame`] using this analyzer's internal clock.
    pub fn latest_frame(&self) -> AudioFrame {
        self.frame(self.started.elapsed().as_secs_f32())
    }

    /// Stop the background capture thread if running and clear live state.
    pub fn stop(&mut self) {
        self.stop_flag.store(true, Ordering::Relaxed);
        if let Some(handle) = self.worker.take() {
            let _ = handle.join();
        }
        if let Ok(mut state) = self.shared.lock() {
            state.frame = AudioFrame::default();
            state.last_packet = None;
            state.capturing = false;
        }
    }

    fn sync_worker(&mut self) {
        let want_live = self.active && !self.test_beat;
        if want_live {
            if self.worker.as_ref().is_some_and(|h| !h.is_finished()) {
                return;
            }
            if let Some(old) = self.worker.take() {
                let _ = old.join();
            }
            let stop = Arc::new(AtomicBool::new(false));
            self.stop_flag = Arc::clone(&stop);
            let shared = Arc::clone(&self.shared);
            let started = self.started;
            self.worker = thread::Builder::new()
                .name("airglow-audio".to_string())
                .spawn(move || run_audio_worker(stop, shared, started))
                .ok();
        } else {
            self.stop();
        }
    }
}

impl Drop for AudioAnalyzer {
    fn drop(&mut self) {
        self.stop();
    }
}

// -----------------------------------------------------------------------------
// InputContext passed into effects::render_zone
// -----------------------------------------------------------------------------

/// Per-frame snapshot of active screen and audio inputs consumed by reactive effects and Studio layers.
#[derive(Debug, Clone, Default)]
pub struct InputContext {
    pub screen: Option<ScreenFrame>,
    pub audio: Option<AudioFrame>,
}

impl InputContext {
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn new(screen: Option<ScreenFrame>, audio: Option<AudioFrame>) -> Self {
        Self { screen, audio }
    }

    /// Update the live [`ScreenSampler`] and [`AudioAnalyzer`] according to what the current scene
    /// needs and return a fresh [`InputContext`] for this render frame.
    pub fn capture(
        screen: &mut ScreenSampler,
        need_screen: bool,
        display: u8,
        audio: &mut AudioAnalyzer,
        need_audio: bool,
        t: f32,
    ) -> Self {
        screen.set_display(display);
        screen.set_active(need_screen);
        if need_screen {
            screen.refresh_if_due();
        }

        audio.set_active(need_audio);
        let audio_frame = if need_audio || audio.is_test_beat() {
            Some(audio.frame(t))
        } else {
            None
        };

        Self {
            screen: if need_screen { screen.frame() } else { None },
            audio: audio_frame,
        }
    }

    /// Sample the captured screen at `norm_pos = [x, y]` (`0.0..=1.0`), optionally horizontally mirrored.
    pub fn sample_screen(&self, norm_pos: [f32; 2], mirror: bool) -> [f32; 3] {
        self.screen
            .as_ref()
            .map(|f| f.sample(norm_pos, mirror))
            .unwrap_or([0.0, 0.0, 0.0])
    }

    /// Sample the captured screen and apply saturation boost (`0..=100`).
    pub fn sample_screen_boosted(
        &self,
        norm_pos: [f32; 2],
        mirror: bool,
        saturation: u8,
    ) -> [f32; 3] {
        self.screen
            .as_ref()
            .map(|f| f.sample_boosted(norm_pos, mirror, saturation))
            .unwrap_or([0.0, 0.0, 0.0])
    }

    /// Sample the 24-band audio spectrum at normalized position `norm_x` (`0.0` = bass, `1.0` = treble).
    pub fn audio_band(&self, norm_x: f32) -> f32 {
        self.audio.map(|a| a.band_at(norm_x)).unwrap_or(0.0)
    }

    /// Return current bass/beat energy (`0.0..=1.0`), or `0.0` if audio is inactive.
    pub fn audio_bass(&self) -> f32 {
        self.audio.map(|a| a.bass).unwrap_or(0.0)
    }

    /// Return mean spectrum level (`0.0..=1.0`), or `0.0` if audio is inactive.
    pub fn audio_level(&self) -> f32 {
        self.audio.map(|a| a.level).unwrap_or(0.0)
    }

    /// Return `true` on a beat onset, or `false` if audio is inactive.
    pub fn audio_beat(&self) -> bool {
        self.audio.map(|a| a.beat).unwrap_or(false)
    }

    /// Return the active [`AudioFrame`], or silence if none is present.
    pub fn audio_frame(&self) -> AudioFrame {
        self.audio.unwrap_or_default()
    }
}

// -----------------------------------------------------------------------------
// Compact pure-Rust radix-2 FFT & math helpers
// -----------------------------------------------------------------------------

#[inline]
fn clamp01(v: f32) -> f32 {
    if v.is_finite() {
        v.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

fn build_log_band_bins(
    sample_rate: u32,
    fft_size: usize,
    band_count: usize,
    f_min: f32,
    f_max: f32,
) -> Vec<(usize, usize)> {
    let max_bin = fft_size / 2;
    let bin_hz = sample_rate as f32 / fft_size as f32;
    let ratio = f_max / f_min;
    let mut out = Vec::with_capacity(band_count);

    for i in 0..band_count {
        let lo_hz = f_min * ratio.powf(i as f32 / band_count as f32);
        let hi_hz = f_min * ratio.powf((i + 1) as f32 / band_count as f32);

        let mut lo_bin = (lo_hz / bin_hz).ceil() as usize;
        let mut hi_bin = ((hi_hz / bin_hz).floor() as usize).min(max_bin);

        if lo_bin > hi_bin || lo_bin > max_bin {
            let center_hz = (lo_hz * hi_hz).sqrt();
            let nearest = ((center_hz / bin_hz).round() as usize).clamp(1, max_bin);
            lo_bin = nearest;
            hi_bin = nearest;
        }
        out.push((lo_bin.min(max_bin), hi_bin.min(max_bin)));
    }
    out
}

/// In-place iterative radix-2 Cooley-Tukey FFT (`re.len() == im.len()`, power of 2).
fn fft_radix2(re: &mut [f32], im: &mut [f32]) {
    let n = re.len();
    debug_assert_eq!(n, im.len());
    debug_assert!(n.is_power_of_two());

    // Bit-reversal permutation.
    let mut j = 0usize;
    for i in 1..n {
        let mut bit = n >> 1;
        while (j & bit) != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j ^= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }

    // Butterfly stages.
    let mut len = 2usize;
    while len <= n {
        let half = len >> 1;
        let angle = -2.0 * PI / len as f32;
        let wlen_re = angle.cos();
        let wlen_im = angle.sin();
        for start in (0..n).step_by(len) {
            let mut w_re = 1.0f32;
            let mut w_im = 0.0f32;
            for k in 0..half {
                let u_idx = start + k;
                let v_idx = u_idx + half;
                let t_re = re[v_idx] * w_re - im[v_idx] * w_im;
                let t_im = re[v_idx] * w_im + im[v_idx] * w_re;
                let u_re = re[u_idx];
                let u_im = im[u_idx];
                re[u_idx] = u_re + t_re;
                im[u_idx] = u_im + t_im;
                re[v_idx] = u_re - t_re;
                im[v_idx] = u_im - t_im;

                let next_re = w_re * wlen_re - w_im * wlen_im;
                let next_im = w_re * wlen_im + w_im * wlen_re;
                w_re = next_re;
                w_im = next_im;
            }
        }
        len <<= 1;
    }
}

// -----------------------------------------------------------------------------
// Platform backends: Win32 GDI screen capture & WASAPI loopback audio capture
// -----------------------------------------------------------------------------

#[cfg(windows)]
fn capture_screen_grid(display: u8, width: usize, height: usize) -> Result<Vec<[f32; 3]>, String> {
    win_screen::capture_grid(display, width, height)
}

#[cfg(not(windows))]
fn capture_screen_grid(_display: u8, _width: usize, _height: usize) -> Result<Vec<[f32; 3]>, String> {
    Err("screen capture is only supported on Windows".to_string())
}

#[cfg(windows)]
fn run_audio_worker(stop: Arc<AtomicBool>, shared: Arc<Mutex<AudioSharedState>>, started: Instant) {
    win_audio::run_loopback_worker(stop, shared, started);
}

#[cfg(not(windows))]
fn run_audio_worker(_stop: Arc<AtomicBool>, shared: Arc<Mutex<AudioSharedState>>, _started: Instant) {
    if let Ok(mut state) = shared.lock() {
        state.capturing = false;
        state.error = Some("system audio loopback is only supported on Windows".to_string());
    }
}

#[cfg(windows)]
mod win_screen {
    use std::ffi::c_void;
    use std::mem::size_of;
    use std::ptr::{null, null_mut};

    type Hwnd = *mut c_void;
    type Hdc = *mut c_void;
    type Hbitmap = *mut c_void;
    type Hgdiobj = *mut c_void;
    type Hmonitor = *mut c_void;
    type Bool = i32;

    const SM_CXSCREEN: i32 = 0;
    const SM_CYSCREEN: i32 = 1;
    const SM_XVIRTUALSCREEN: i32 = 76;
    const SM_YVIRTUALSCREEN: i32 = 77;
    const SM_CXVIRTUALSCREEN: i32 = 78;
    const SM_CYVIRTUALSCREEN: i32 = 79;

    const HALFTONE: i32 = 4;
    const SRCCOPY: u32 = 0x00CC_0020;
    const BI_RGB: u32 = 0;
    const DIB_RGB_COLORS: u32 = 0;
    const MONITORINFOF_PRIMARY: u32 = 1;

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct Rect {
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
    }

    #[repr(C)]
    struct MonitorInfo {
        cb_size: u32,
        rc_monitor: Rect,
        rc_work: Rect,
        dw_flags: u32,
    }

    #[repr(C)]
    struct BitmapInfoHeader {
        bi_size: u32,
        bi_width: i32,
        bi_height: i32,
        bi_planes: u16,
        bi_bit_count: u16,
        bi_compression: u32,
        bi_size_image: u32,
        bi_x_pels_per_meter: i32,
        bi_y_pels_per_meter: i32,
        bi_clr_used: u32,
        bi_clr_important: u32,
    }

    #[repr(C)]
    struct BitmapInfo {
        bmi_header: BitmapInfoHeader,
        bmi_colors: [u32; 1],
    }

    type MonitorEnumProc =
        unsafe extern "system" fn(Hmonitor, Hdc, *mut Rect, isize) -> Bool;

    #[link(name = "user32")]
    unsafe extern "system" {
        fn GetDC(hwnd: Hwnd) -> Hdc;
        fn ReleaseDC(hwnd: Hwnd, hdc: Hdc) -> i32;
        fn GetSystemMetrics(index: i32) -> i32;
        fn EnumDisplayMonitors(
            hdc: Hdc,
            lprc_clip: *const Rect,
            lpfn_enum: MonitorEnumProc,
            dw_data: isize,
        ) -> Bool;
        fn GetMonitorInfoW(hmonitor: Hmonitor, lpmi: *mut MonitorInfo) -> Bool;
    }

    #[link(name = "gdi32")]
    unsafe extern "system" {
        fn CreateCompatibleDC(hdc: Hdc) -> Hdc;
        fn DeleteDC(hdc: Hdc) -> Bool;
        fn CreateCompatibleBitmap(hdc: Hdc, cx: i32, cy: i32) -> Hbitmap;
        fn SelectObject(hdc: Hdc, h: Hgdiobj) -> Hgdiobj;
        fn DeleteObject(ho: Hgdiobj) -> Bool;
        fn SetStretchBltMode(hdc: Hdc, mode: i32) -> i32;
        fn SetBrushOrgEx(hdc: Hdc, x: i32, y: i32, lppt: *mut c_void) -> Bool;
        fn StretchBlt(
            hdc_dest: Hdc,
            x_dest: i32,
            y_dest: i32,
            w_dest: i32,
            h_dest: i32,
            hdc_src: Hdc,
            x_src: i32,
            y_src: i32,
            w_src: i32,
            h_src: i32,
            rop: u32,
        ) -> Bool;
        fn GetDIBits(
            hdc: Hdc,
            hbm: Hbitmap,
            start: u32,
            c_lines: u32,
            lpv_bits: *mut c_void,
            lpbmi: *mut BitmapInfo,
            usage: u32,
        ) -> i32;
    }

    unsafe extern "system" fn enum_monitor_cb(
        hmon: Hmonitor,
        _hdc: Hdc,
        _rect: *mut Rect,
        lparam: isize,
    ) -> Bool {
        let list = unsafe { &mut *(lparam as *mut Vec<(bool, Rect)>) };
        let mut info = MonitorInfo {
            cb_size: size_of::<MonitorInfo>() as u32,
            rc_monitor: Rect::default(),
            rc_work: Rect::default(),
            dw_flags: 0,
        };
        if unsafe { GetMonitorInfoW(hmon, &mut info) } != 0 {
            let is_primary = (info.dw_flags & MONITORINFOF_PRIMARY) != 0;
            list.push((is_primary, info.rc_monitor));
        }
        1
    }

    fn resolve_display_rect(display: u8) -> Rect {
        unsafe {
            if display == 0 {
                let x = GetSystemMetrics(SM_XVIRTUALSCREEN);
                let y = GetSystemMetrics(SM_YVIRTUALSCREEN);
                let w = GetSystemMetrics(SM_CXVIRTUALSCREEN);
                let h = GetSystemMetrics(SM_CYVIRTUALSCREEN);
                if w > 0 && h > 0 {
                    return Rect {
                        left: x,
                        top: y,
                        right: x + w,
                        bottom: y + h,
                    };
                }
            } else if display > 1 {
                let mut monitors: Vec<(bool, Rect)> = Vec::new();
                let ptr = (&mut monitors as *mut Vec<(bool, Rect)>) as isize;
                let _ = EnumDisplayMonitors(null_mut(), null(), enum_monitor_cb, ptr);
                // Put primary monitor first (display == 1), then remaining displays.
                monitors.sort_by_key(|(primary, _)| !*primary);
                let idx = (display - 1) as usize;
                if let Some((_, rc)) = monitors.get(idx)
                    && rc.right > rc.left
                    && rc.bottom > rc.top
                {
                    return *rc;
                }
            }

            let w = GetSystemMetrics(SM_CXSCREEN).max(1);
            let h = GetSystemMetrics(SM_CYSCREEN).max(1);
            Rect {
                left: 0,
                top: 0,
                right: w,
                bottom: h,
            }
        }
    }

    pub fn capture_grid(display: u8, width: usize, height: usize) -> Result<Vec<[f32; 3]>, String> {
        let rect = resolve_display_rect(display);
        let src_w = rect.right - rect.left;
        let src_h = rect.bottom - rect.top;
        if src_w <= 0 || src_h <= 0 {
            return Err("invalid display dimensions".to_string());
        }

        unsafe {
            let screen_dc = GetDC(null_mut());
            if screen_dc.is_null() {
                return Err("GetDC(NULL) failed".to_string());
            }

            let mem_dc = CreateCompatibleDC(screen_dc);
            if mem_dc.is_null() {
                ReleaseDC(null_mut(), screen_dc);
                return Err("CreateCompatibleDC failed".to_string());
            }

            let bmp = CreateCompatibleBitmap(screen_dc, width as i32, height as i32);
            if bmp.is_null() {
                DeleteDC(mem_dc);
                ReleaseDC(null_mut(), screen_dc);
                return Err("CreateCompatibleBitmap failed".to_string());
            }

            let old_obj = SelectObject(mem_dc, bmp);
            SetStretchBltMode(mem_dc, HALFTONE);
            SetBrushOrgEx(mem_dc, 0, 0, null_mut());

            let blt_ok = StretchBlt(
                mem_dc,
                0,
                0,
                width as i32,
                height as i32,
                screen_dc,
                rect.left,
                rect.top,
                src_w,
                src_h,
                SRCCOPY,
            );

            let mut bgra = vec![0u8; width * height * 4];
            let mut bmi = BitmapInfo {
                bmi_header: BitmapInfoHeader {
                    bi_size: size_of::<BitmapInfoHeader>() as u32,
                    bi_width: width as i32,
                    // Negative height requests a top-down DIB so row 0 is the top of the screen.
                    bi_height: -(height as i32),
                    bi_planes: 1,
                    bi_bit_count: 32,
                    bi_compression: BI_RGB,
                    bi_size_image: 0,
                    bi_x_pels_per_meter: 0,
                    bi_y_pels_per_meter: 0,
                    bi_clr_used: 0,
                    bi_clr_important: 0,
                },
                bmi_colors: [0],
            };

            let lines = if blt_ok != 0 {
                GetDIBits(
                    mem_dc,
                    bmp,
                    0,
                    height as u32,
                    bgra.as_mut_ptr() as *mut c_void,
                    &mut bmi,
                    DIB_RGB_COLORS,
                )
            } else {
                0
            };

            SelectObject(mem_dc, old_obj);
            DeleteObject(bmp);
            DeleteDC(mem_dc);
            ReleaseDC(null_mut(), screen_dc);

            if blt_ok == 0 || lines == 0 {
                return Err("StretchBlt/GetDIBits failed (desktop may be locked)".to_string());
            }

            let mut rgb = Vec::with_capacity(width * height);
            for px in bgra.chunks_exact(4) {
                rgb.push([
                    px[2] as f32 / 255.0,
                    px[1] as f32 / 255.0,
                    px[0] as f32 / 255.0,
                ]);
            }
            Ok(rgb)
        }
    }
}

#[cfg(windows)]
mod win_audio {
    use super::{AudioSharedState, DEFAULT_SAMPLE_RATE, FFT_SIZE, SpectrumProcessor};
    use std::collections::VecDeque;
    use std::ffi::c_void;
    use std::ptr::{null, null_mut};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::{Duration, Instant};

    type Hresult = i32;

    const S_OK: Hresult = 0;
    const S_FALSE: Hresult = 1;
    const RPC_E_CHANGED_MODE: Hresult = 0x8001_0106u32 as i32;
    const AUDCLNT_E_DEVICE_INVALIDATED: Hresult = 0x8889_0004u32 as i32;

    const COINIT_MULTITHREADED: u32 = 0x0;
    const CLSCTX_ALL: u32 = 0x17;
    const E_RENDER: u32 = 0;
    const E_CONSOLE: u32 = 0;
    const AUDCLNT_SHAREMODE_SHARED: u32 = 0;
    const AUDCLNT_STREAMFLAGS_LOOPBACK: u32 = 0x0002_0000;
    const AUDCLNT_BUFFERFLAGS_SILENT: u32 = 0x0000_0002;

    const WAVE_FORMAT_PCM: u16 = 1;
    const WAVE_FORMAT_IEEE_FLOAT: u16 = 3;
    const WAVE_FORMAT_EXTENSIBLE: u16 = 0xFFFE;

    const DEVICE_RECHECK_INTERVAL: Duration = Duration::from_secs(3);

    #[repr(C)]
    #[derive(Clone, Copy, PartialEq, Eq)]
    struct Guid {
        data1: u32,
        data2: u16,
        data3: u16,
        data4: [u8; 8],
    }

    const CLSID_MM_DEVICE_ENUMERATOR: Guid = Guid {
        data1: 0xBCDE_0395,
        data2: 0xE52F,
        data3: 0x467C,
        data4: [0x8E, 0x3D, 0xC4, 0x57, 0x92, 0x91, 0x69, 0x2E],
    };
    const IID_IMM_DEVICE_ENUMERATOR: Guid = Guid {
        data1: 0xA956_64D2,
        data2: 0x9614,
        data3: 0x4F35,
        data4: [0xA7, 0x46, 0xDE, 0x8D, 0xB6, 0x36, 0x17, 0xE6],
    };
    const IID_I_AUDIO_CLIENT: Guid = Guid {
        data1: 0x1CB9_AD4C,
        data2: 0xDBFA,
        data3: 0x4C32,
        data4: [0xB1, 0x78, 0xC2, 0xF5, 0x68, 0xA7, 0x03, 0xB2],
    };
    const IID_I_AUDIO_CAPTURE_CLIENT: Guid = Guid {
        data1: 0xC8AD_BD64,
        data2: 0xE71E,
        data3: 0x48A0,
        data4: [0xA4, 0xDE, 0x18, 0x5C, 0x39, 0x5C, 0xD3, 0x17],
    };
    const KSDATAFORMAT_SUBTYPE_IEEE_FLOAT: Guid = Guid {
        data1: 0x0000_0003,
        data2: 0x0000,
        data3: 0x0010,
        data4: [0x80, 0x00, 0x00, 0xAA, 0x00, 0x38, 0x9B, 0x71],
    };
    const KSDATAFORMAT_SUBTYPE_PCM: Guid = Guid {
        data1: 0x0000_0001,
        data2: 0x0000,
        data3: 0x0010,
        data4: [0x80, 0x00, 0x00, 0xAA, 0x00, 0x38, 0x9B, 0x71],
    };

    #[repr(C, packed)]
    #[derive(Clone, Copy)]
    struct WaveFormatEx {
        w_format_tag: u16,
        n_channels: u16,
        n_samples_per_sec: u32,
        n_avg_bytes_per_sec: u32,
        n_block_align: u16,
        w_bits_per_sample: u16,
        cb_size: u16,
    }

    #[repr(C, packed)]
    #[derive(Clone, Copy)]
    struct WaveFormatExtensible {
        format: WaveFormatEx,
        w_valid_bits_per_sample: u16,
        dw_channel_mask: u32,
        sub_format: Guid,
    }

    // Minimal COM vtable declarations verified against Windows SDK mmdeviceapi.h & Audioclient.h.

    #[repr(C)]
    struct IUnknownVtbl {
        _query_interface: usize,
        _add_ref: usize,
        release: unsafe extern "system" fn(*mut c_void) -> u32,
    }

    #[repr(C)]
    struct IMMDeviceEnumerator {
        lp_vtbl: *const IMMDeviceEnumeratorVtbl,
    }

    #[repr(C)]
    struct IMMDeviceEnumeratorVtbl {
        base: IUnknownVtbl,
        _enum_audio_endpoints: usize,
        get_default_audio_endpoint: unsafe extern "system" fn(
            *mut IMMDeviceEnumerator,
            u32,
            u32,
            *mut *mut IMMDevice,
        ) -> Hresult,
    }

    #[repr(C)]
    struct IMMDevice {
        lp_vtbl: *const IMMDeviceVtbl,
    }

    #[repr(C)]
    struct IMMDeviceVtbl {
        base: IUnknownVtbl,
        activate: unsafe extern "system" fn(
            *mut IMMDevice,
            *const Guid,
            u32,
            *mut c_void,
            *mut *mut c_void,
        ) -> Hresult,
        _open_property_store: usize,
        get_id: unsafe extern "system" fn(*mut IMMDevice, *mut *mut u16) -> Hresult,
    }

    #[repr(C)]
    struct IAudioClient {
        lp_vtbl: *const IAudioClientVtbl,
    }

    #[repr(C)]
    struct IAudioClientVtbl {
        base: IUnknownVtbl,
        initialize: unsafe extern "system" fn(
            *mut IAudioClient,
            u32,
            u32,
            i64,
            i64,
            *const WaveFormatEx,
            *const Guid,
        ) -> Hresult,
        _get_buffer_size: usize,
        _get_stream_latency: usize,
        _get_current_padding: usize,
        _is_format_supported: usize,
        get_mix_format:
            unsafe extern "system" fn(*mut IAudioClient, *mut *mut WaveFormatEx) -> Hresult,
        _get_device_period: usize,
        start: unsafe extern "system" fn(*mut IAudioClient) -> Hresult,
        stop: unsafe extern "system" fn(*mut IAudioClient) -> Hresult,
        _reset: usize,
        _set_event_handle: usize,
        get_service:
            unsafe extern "system" fn(*mut IAudioClient, *const Guid, *mut *mut c_void) -> Hresult,
    }

    #[repr(C)]
    struct IAudioCaptureClient {
        lp_vtbl: *const IAudioCaptureClientVtbl,
    }

    #[repr(C)]
    struct IAudioCaptureClientVtbl {
        base: IUnknownVtbl,
        get_buffer: unsafe extern "system" fn(
            *mut IAudioCaptureClient,
            *mut *const u8,
            *mut u32,
            *mut u32,
            *mut u64,
            *mut u64,
        ) -> Hresult,
        release_buffer: unsafe extern "system" fn(*mut IAudioCaptureClient, u32) -> Hresult,
        get_next_packet_size:
            unsafe extern "system" fn(*mut IAudioCaptureClient, *mut u32) -> Hresult,
    }

    #[link(name = "ole32")]
    unsafe extern "system" {
        fn CoInitializeEx(pv_reserved: *mut c_void, dw_co_init: u32) -> Hresult;
        fn CoUninitialize();
        fn CoCreateInstance(
            rclsid: *const Guid,
            p_unk_outer: *mut c_void,
            dw_cls_context: u32,
            riid: *const Guid,
            ppv: *mut *mut c_void,
        ) -> Hresult;
        fn CoTaskMemFree(pv: *mut c_void);
    }

    struct CoInitGuard(bool);
    impl Drop for CoInitGuard {
        fn drop(&mut self) {
            if self.0 {
                unsafe { CoUninitialize() };
            }
        }
    }

    struct ComPtr<T>(*mut T);
    impl<T> ComPtr<T> {
        fn as_ptr(&self) -> *mut T {
            self.0
        }
    }
    impl<T> Drop for ComPtr<T> {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe {
                    let vtbl = *(self.0 as *const *const IUnknownVtbl);
                    ((*vtbl).release)(self.0 as *mut c_void);
                }
            }
        }
    }

    struct CoTaskMemPtr<T>(*mut T);
    impl<T> Drop for CoTaskMemPtr<T> {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe { CoTaskMemFree(self.0 as *mut c_void) };
            }
        }
    }

    #[derive(Clone, Copy)]
    enum SampleFormat {
        F32,
        I16,
        I24,
        I32,
    }

    struct MixFormat {
        sample_rate: u32,
        channels: usize,
        block_align: usize,
        sample_format: SampleFormat,
    }

    fn parse_mix_format(ptr: *const WaveFormatEx) -> Result<MixFormat, String> {
        let wfx = unsafe { ptr.read_unaligned() };
        let tag = wfx.w_format_tag;
        let bits = wfx.w_bits_per_sample;
        let channels = (wfx.n_channels as usize).max(1);
        let sample_rate = wfx.n_samples_per_sec.max(8_000);
        let block_align = (wfx.n_block_align as usize).max(channels * (bits as usize / 8).max(1));

        let is_float = if tag == WAVE_FORMAT_IEEE_FLOAT {
            true
        } else if tag == WAVE_FORMAT_PCM {
            false
        } else if tag == WAVE_FORMAT_EXTENSIBLE && wfx.cb_size >= 22 {
            let ext = unsafe { (ptr as *const WaveFormatExtensible).read_unaligned() };
            let sub = ext.sub_format;
            if sub == KSDATAFORMAT_SUBTYPE_IEEE_FLOAT {
                true
            } else if sub == KSDATAFORMAT_SUBTYPE_PCM {
                false
            } else {
                return Err(format!("unsupported WASAPI extensible format (bits={bits})"));
            }
        } else {
            return Err(format!("unsupported WASAPI format tag 0x{tag:04X}"));
        };

        let sample_format = match (is_float, bits) {
            (true, 32) => SampleFormat::F32,
            (false, 16) => SampleFormat::I16,
            (false, 24) => SampleFormat::I24,
            (false, 32) => SampleFormat::I32,
            _ => return Err(format!("unsupported WASAPI bit depth {bits}")),
        };

        Ok(MixFormat {
            sample_rate,
            channels,
            block_align,
            sample_format,
        })
    }

    fn device_id_string(dev: *mut IMMDevice) -> Option<String> {
        let mut raw_id: *mut u16 = null_mut();
        let hr = unsafe { ((*(*dev).lp_vtbl).get_id)(dev, &mut raw_id) };
        if hr < 0 || raw_id.is_null() {
            return None;
        }
        let _guard = CoTaskMemPtr(raw_id);
        let mut len = 0usize;
        unsafe {
            while *raw_id.add(len) != 0 && len < 1024 {
                len += 1;
            }
            let slice = std::slice::from_raw_parts(raw_id, len);
            Some(String::from_utf16_lossy(slice))
        }
    }

    fn decode_mono_frames(
        data: *const u8,
        frames: usize,
        silent: bool,
        fmt: &MixFormat,
        ring: &mut VecDeque<f32>,
    ) {
        if silent || data.is_null() {
            for _ in 0..frames {
                ring.push_back(0.0);
            }
            return;
        }

        let total_bytes = frames.saturating_mul(fmt.block_align);
        let bytes = unsafe { std::slice::from_raw_parts(data, total_bytes) };
        let inv_ch = 1.0 / fmt.channels as f32;

        for frame_bytes in bytes.chunks_exact(fmt.block_align) {
            let mut sum = 0.0f32;
            match fmt.sample_format {
                SampleFormat::F32 => {
                    for ch in 0..fmt.channels {
                        let off = ch * 4;
                        if off + 4 <= frame_bytes.len() {
                            sum += f32::from_le_bytes([
                                frame_bytes[off],
                                frame_bytes[off + 1],
                                frame_bytes[off + 2],
                                frame_bytes[off + 3],
                            ]);
                        }
                    }
                }
                SampleFormat::I16 => {
                    for ch in 0..fmt.channels {
                        let off = ch * 2;
                        if off + 2 <= frame_bytes.len() {
                            let v = i16::from_le_bytes([frame_bytes[off], frame_bytes[off + 1]]);
                            sum += v as f32 / 32768.0;
                        }
                    }
                }
                SampleFormat::I24 => {
                    for ch in 0..fmt.channels {
                        let off = ch * 3;
                        if off + 3 <= frame_bytes.len() {
                            let raw = ((frame_bytes[off] as i32) << 8)
                                | ((frame_bytes[off + 1] as i32) << 16)
                                | ((frame_bytes[off + 2] as i32) << 24);
                            sum += (raw >> 8) as f32 / 8_388_608.0;
                        }
                    }
                }
                SampleFormat::I32 => {
                    for ch in 0..fmt.channels {
                        let off = ch * 4;
                        if off + 4 <= frame_bytes.len() {
                            let v = i32::from_le_bytes([
                                frame_bytes[off],
                                frame_bytes[off + 1],
                                frame_bytes[off + 2],
                                frame_bytes[off + 3],
                            ]);
                            sum += v as f32 / 2_147_483_648.0;
                        }
                    }
                }
            }
            ring.push_back(sum * inv_ch);
        }
    }

    pub fn run_loopback_worker(
        stop: Arc<AtomicBool>,
        shared: Arc<Mutex<AudioSharedState>>,
        started: Instant,
    ) {
        let hr = unsafe { CoInitializeEx(null_mut(), COINIT_MULTITHREADED) };
        let should_uninit = hr == S_OK || hr == S_FALSE;
        if hr < 0 && hr != RPC_E_CHANGED_MODE {
            if let Ok(mut state) = shared.lock() {
                state.capturing = false;
                state.error = Some(format!("CoInitializeEx failed: 0x{:08X}", hr as u32));
            }
            return;
        }
        let _co_guard = CoInitGuard(should_uninit);

        while !stop.load(Ordering::Relaxed) {
            match run_single_endpoint(&stop, &shared, started) {
                Ok(()) => {
                    // Clean exit (stop requested or default output changed).
                }
                Err(err) => {
                    if let Ok(mut state) = shared.lock() {
                        state.capturing = false;
                        state.error = Some(err);
                    }
                    // Back off briefly before retrying if audio service/device is unavailable.
                    for _ in 0..20 {
                        if stop.load(Ordering::Relaxed) {
                            break;
                        }
                        thread::sleep(Duration::from_millis(50));
                    }
                }
            }
        }

        if let Ok(mut state) = shared.lock() {
            state.capturing = false;
        }
    }

    fn run_single_endpoint(
        stop: &AtomicBool,
        shared: &Mutex<AudioSharedState>,
        started: Instant,
    ) -> Result<(), String> {
        let enumerator = unsafe {
            let mut raw: *mut c_void = null_mut();
            let hr = CoCreateInstance(
                &CLSID_MM_DEVICE_ENUMERATOR,
                null_mut(),
                CLSCTX_ALL,
                &IID_IMM_DEVICE_ENUMERATOR,
                &mut raw,
            );
            if hr < 0 || raw.is_null() {
                return Err(format!("IMMDeviceEnumerator unavailable: 0x{:08X}", hr as u32));
            }
            ComPtr(raw as *mut IMMDeviceEnumerator)
        };

        let device = unsafe {
            let mut raw: *mut IMMDevice = null_mut();
            let hr = ((*(*enumerator.as_ptr()).lp_vtbl).get_default_audio_endpoint)(
                enumerator.as_ptr(),
                E_RENDER,
                E_CONSOLE,
                &mut raw,
            );
            if hr < 0 || raw.is_null() {
                return Err(format!("no default audio output device: 0x{:08X}", hr as u32));
            }
            ComPtr(raw)
        };
        let active_id = device_id_string(device.as_ptr());

        let audio_client = unsafe {
            let mut raw: *mut c_void = null_mut();
            let hr = ((*(*device.as_ptr()).lp_vtbl).activate)(
                device.as_ptr(),
                &IID_I_AUDIO_CLIENT,
                CLSCTX_ALL,
                null_mut(),
                &mut raw,
            );
            if hr < 0 || raw.is_null() {
                return Err(format!("IAudioClient::Activate failed: 0x{:08X}", hr as u32));
            }
            ComPtr(raw as *mut IAudioClient)
        };

        let mix_ptr = unsafe {
            let mut raw: *mut WaveFormatEx = null_mut();
            let hr = ((*(*audio_client.as_ptr()).lp_vtbl).get_mix_format)(
                audio_client.as_ptr(),
                &mut raw,
            );
            if hr < 0 || raw.is_null() {
                return Err(format!("GetMixFormat failed: 0x{:08X}", hr as u32));
            }
            CoTaskMemPtr(raw)
        };
        let mix_fmt = parse_mix_format(mix_ptr.0)?;

        // 100 ms buffer in 100-ns reference time units.
        let hr = unsafe {
            ((*(*audio_client.as_ptr()).lp_vtbl).initialize)(
                audio_client.as_ptr(),
                AUDCLNT_SHAREMODE_SHARED,
                AUDCLNT_STREAMFLAGS_LOOPBACK,
                1_000_000,
                0,
                mix_ptr.0,
                null(),
            )
        };
        if hr < 0 {
            return Err(format!(
                "IAudioClient::Initialize(LOOPBACK) failed: 0x{:08X}",
                hr as u32
            ));
        }

        let capture_client = unsafe {
            let mut raw: *mut c_void = null_mut();
            let hr = ((*(*audio_client.as_ptr()).lp_vtbl).get_service)(
                audio_client.as_ptr(),
                &IID_I_AUDIO_CAPTURE_CLIENT,
                &mut raw,
            );
            if hr < 0 || raw.is_null() {
                return Err(format!(
                    "GetService(IAudioCaptureClient) failed: 0x{:08X}",
                    hr as u32
                ));
            }
            ComPtr(raw as *mut IAudioCaptureClient)
        };

        let hr = unsafe { ((*(*audio_client.as_ptr()).lp_vtbl).start)(audio_client.as_ptr()) };
        if hr < 0 {
            return Err(format!("IAudioClient::Start failed: 0x{:08X}", hr as u32));
        }

        if let Ok(mut state) = shared.lock() {
            state.capturing = true;
            state.error = None;
        }

        let mut processor = SpectrumProcessor::new(mix_fmt.sample_rate.max(DEFAULT_SAMPLE_RATE / 6));
        let mut ring: VecDeque<f32> = VecDeque::with_capacity(FFT_SIZE * 4);
        let mut block = [0.0f32; FFT_SIZE];
        let mut last_device_check = Instant::now();

        let result = loop {
            if stop.load(Ordering::Relaxed) {
                break Ok(());
            }

            if last_device_check.elapsed() >= DEVICE_RECHECK_INTERVAL {
                last_device_check = Instant::now();
                if let Some(ref cur_id) = active_id {
                    let mut raw_def: *mut IMMDevice = null_mut();
                    let hr = unsafe {
                        ((*(*enumerator.as_ptr()).lp_vtbl).get_default_audio_endpoint)(
                            enumerator.as_ptr(),
                            E_RENDER,
                            E_CONSOLE,
                            &mut raw_def,
                        )
                    };
                    if hr >= 0 && !raw_def.is_null() {
                        let def_dev = ComPtr(raw_def);
                        if device_id_string(def_dev.as_ptr()).as_deref() != Some(cur_id.as_str()) {
                            // Default output changed (e.g. headphones plugged in): re-open.
                            break Ok(());
                        }
                    }
                }
            }

            let mut packet_frames: u32 = 0;
            let hr = unsafe {
                ((*(*capture_client.as_ptr()).lp_vtbl).get_next_packet_size)(
                    capture_client.as_ptr(),
                    &mut packet_frames,
                )
            };
            if hr == AUDCLNT_E_DEVICE_INVALIDATED {
                break Ok(());
            } else if hr < 0 {
                break Err(format!("GetNextPacketSize failed: 0x{:08X}", hr as u32));
            }

            if packet_frames == 0 {
                thread::sleep(Duration::from_millis(10));
                continue;
            }

            while packet_frames > 0 && !stop.load(Ordering::Relaxed) {
                let mut data_ptr: *const u8 = null();
                let mut num_frames: u32 = 0;
                let mut flags: u32 = 0;
                let hr = unsafe {
                    ((*(*capture_client.as_ptr()).lp_vtbl).get_buffer)(
                        capture_client.as_ptr(),
                        &mut data_ptr,
                        &mut num_frames,
                        &mut flags,
                        null_mut(),
                        null_mut(),
                    )
                };
                if hr == AUDCLNT_E_DEVICE_INVALIDATED {
                    let _ = unsafe {
                        ((*(*audio_client.as_ptr()).lp_vtbl).stop)(audio_client.as_ptr())
                    };
                    return Ok(());
                } else if hr < 0 {
                    let _ = unsafe {
                        ((*(*audio_client.as_ptr()).lp_vtbl).stop)(audio_client.as_ptr())
                    };
                    return Err(format!("GetBuffer failed: 0x{:08X}", hr as u32));
                }

                let silent = (flags & AUDCLNT_BUFFERFLAGS_SILENT) != 0;
                decode_mono_frames(data_ptr, num_frames as usize, silent, &mix_fmt, &mut ring);

                let _ = unsafe {
                    ((*(*capture_client.as_ptr()).lp_vtbl).release_buffer)(
                        capture_client.as_ptr(),
                        num_frames,
                    )
                };

                // Bound queue growth if packets arrive faster than consumed.
                while ring.len() > FFT_SIZE * 4 {
                    ring.pop_front();
                }

                while ring.len() >= FFT_SIZE {
                    for slot in &mut block {
                        *slot = ring.pop_front().unwrap_or(0.0);
                    }
                    let now_secs = started.elapsed().as_secs_f32();
                    let frame = processor.process_mono(&block, now_secs);
                    if let Ok(mut state) = shared.lock() {
                        state.frame = frame;
                        state.last_packet = Some(Instant::now());
                    }
                }

                packet_frames = 0;
                let hr = unsafe {
                    ((*(*capture_client.as_ptr()).lp_vtbl).get_next_packet_size)(
                        capture_client.as_ptr(),
                        &mut packet_frames,
                    )
                };
                if hr < 0 {
                    break;
                }
            }
        };

        let _ = unsafe { ((*(*audio_client.as_ptr()).lp_vtbl).stop)(audio_client.as_ptr()) };
        result
    }
}

// -----------------------------------------------------------------------------
// Unit tests
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn gradient_screen_frame() -> ScreenFrame {
        // 4x2 grid where R = x / 3 and G = y, B = 0.25.
        let mut pixels = Vec::with_capacity(8);
        for y in 0..2 {
            for x in 0..4 {
                pixels.push([x as f32 / 3.0, y as f32, 0.25]);
            }
        }
        ScreenFrame::from_grid(4, 2, pixels)
    }

    #[test]
    fn screen_sampling_and_mirroring() {
        let sampler = ScreenSampler::from_frame(gradient_screen_frame());

        // Top-left (0, 0) -> R=0.0, G=0.0, B=0.25
        let tl = sampler.sample([0.0, 0.0], false);
        assert!((tl[0] - 0.0).abs() < 1e-5);
        assert!((tl[1] - 0.0).abs() < 1e-5);
        assert!((tl[2] - 0.25).abs() < 1e-5);

        // Top-right (1, 0) -> R=1.0, G=0.0
        let tr = sampler.sample([1.0, 0.0], false);
        assert!((tr[0] - 1.0).abs() < 1e-5);
        assert!((tr[1] - 0.0).abs() < 1e-5);

        // Mirrored top-left samples the top-right pixel!
        let tl_mirrored = sampler.sample([0.0, 0.0], true);
        assert!((tl_mirrored[0] - 1.0).abs() < 1e-5);
        assert!((tl_mirrored[1] - 0.0).abs() < 1e-5);

        // Mirrored bottom-right samples the bottom-left pixel (R=0.0, G=1.0).
        let br_mirrored = sampler.sample([1.0, 1.0], true);
        assert!((br_mirrored[0] - 0.0).abs() < 1e-5);
        assert!((br_mirrored[1] - 1.0).abs() < 1e-5);
    }

    #[test]
    fn screen_sampling_bounds_clamping_and_inactive_empty() {
        let empty = ScreenSampler::new();
        assert!(!empty.is_active());
        assert_eq!(empty.sample([0.5, 0.5], false), [0.0, 0.0, 0.0]);

        let sampler = ScreenSampler::from_frame(gradient_screen_frame());
        // Out-of-bounds negative and >1 values clamp cleanly without panicking.
        assert_eq!(
            sampler.sample([-5.0, -2.0], false),
            sampler.sample([0.0, 0.0], false)
        );
        assert_eq!(
            sampler.sample([9.0, 4.0], false),
            sampler.sample([1.0, 1.0], false)
        );
        // Non-finite inputs clamp safely to 0.0.
        assert_eq!(
            sampler.sample([f32::NAN, f32::INFINITY], false),
            sampler.sample([0.0, 0.0], false)
        );

        // Saturation boost stays bounded to 0..=1.
        let ctx = InputContext::new(sampler.frame(), None);
        let boosted = ctx.sample_screen_boosted([1.0, 0.0], false, 100);
        for c in boosted {
            assert!((0.0..=1.0).contains(&c));
        }
        assert!(boosted[0] >= 0.99);
    }

    #[test]
    fn fft_spectrum_localizes_bass_and_treble_and_normalizes() {
        let mut proc = SpectrumProcessor::new(48_000);

        // 1) Pure 100 Hz bass sine wave.
        let bass_wave: Vec<f32> = (0..FFT_SIZE)
            .map(|i| (2.0 * PI * 100.0 * i as f32 / 48_000.0).sin())
            .collect();
        let bass_frame = proc.process_mono(&bass_wave, 0.5);

        for &b in &bass_frame.bands {
            assert!(b.is_finite() && (0.0..=1.0).contains(&b));
        }
        assert!((0.0..=1.0).contains(&bass_frame.bass));
        assert!((0.0..=1.0).contains(&bass_frame.level));

        let low_energy: f32 = bass_frame.bands[0..6].iter().sum();
        let high_energy: f32 = bass_frame.bands[16..24].iter().sum();
        assert!(
            low_energy > high_energy * 4.0,
            "expected bass energy ({low_energy}) to dominate treble ({high_energy})"
        );
        assert!(bass_frame.bass > 0.2);

        // 2) Pure 6000 Hz treble sine wave on a fresh processor.
        let mut proc_hi = SpectrumProcessor::new(48_000);
        let treble_wave: Vec<f32> = (0..FFT_SIZE)
            .map(|i| (2.0 * PI * 6_000.0 * i as f32 / 48_000.0).sin())
            .collect();
        let treble_frame = proc_hi.process_mono(&treble_wave, 0.5);
        let treble_low: f32 = treble_frame.bands[0..6].iter().sum();
        let treble_high: f32 = treble_frame.bands[16..24].iter().sum();
        assert!(
            treble_high > treble_low * 4.0,
            "expected treble energy ({treble_high}) to dominate bass ({treble_low})"
        );

        // 3) Silence decays smoothly toward 0 without NaNs.
        let decayed = proc.decay();
        assert!(decayed.bass < bass_frame.bass);
        assert!(!decayed.beat);
    }

    #[test]
    fn test_beat_generates_120_bpm_pulses_and_spectrum() {
        let mut analyzer = AudioAnalyzer::new();
        analyzer.set_test_beat(true);
        assert!(analyzer.is_test_beat());

        // Right on the beat (t = 0.0, 0.5, 1.0 s at 120 BPM).
        let on_beat_0 = analyzer.frame(0.0);
        let between_beats = analyzer.frame(0.35);
        let on_beat_1 = analyzer.frame(0.5);

        assert!(on_beat_0.beat, "expected beat flag at t=0.0");
        assert!(on_beat_1.beat, "expected beat flag at t=0.5 (120 BPM)");
        assert!(!between_beats.beat, "expected no beat flag at t=0.35");

        assert!(
            on_beat_0.bass > 0.9 && on_beat_1.bass > 0.9,
            "expected strong bass kick on beat"
        );
        assert!(
            between_beats.bass < 0.2,
            "expected bass decay between beats, got {}",
            between_beats.bass
        );

        for f in [on_beat_0, between_beats, on_beat_1] {
            for &b in &f.bands {
                assert!(b.is_finite() && (0.0..=1.0).contains(&b));
            }
            assert!((0.0..=1.0).contains(&f.level));
        }

        // InputContext helper wiring works cleanly.
        let ctx = InputContext::new(None, Some(on_beat_0));
        assert!(ctx.audio_beat());
        assert!((ctx.audio_bass() - on_beat_0.bass).abs() < 1e-6);
        assert!((ctx.audio_band(0.0) - on_beat_0.bands[0]).abs() < 1e-6);
        assert!((ctx.audio_band(1.0) - on_beat_0.bands[AUDIO_BANDS - 1]).abs() < 1e-6);
    }
}
