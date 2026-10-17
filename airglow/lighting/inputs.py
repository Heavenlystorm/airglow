"""Screen and audio inputs for reactive effects. Each runs in its own thread, only while an effect uses it,
and stops a few seconds after the last use, so nothing is captured when no reactive effect is on."""

import logging
import threading
import time
import warnings

import numpy as np

log = logging.getLogger(__name__)


class _LazyWorker:
    IDLE_STOP = 5.0          # seconds without a request before the thread stops

    def __init__(self):
        self._thread = None
        self._last_use = 0.0
        self._lock = threading.Lock()
        self.error = None

    def touch(self, **options):
        """Mark the input as in use, starting its thread if needed. options update the worker's settings."""
        for k, v in options.items():
            setattr(self, k, v)
        self._last_use = time.monotonic()
        with self._lock:
            if self._thread is None or not self._thread.is_alive():
                self._thread = threading.Thread(target=self._guarded, name=type(self).__name__, daemon=True)
                self._thread.start()

    @property
    def active(self):
        return self._thread is not None and self._thread.is_alive()

    def _should_run(self):
        return time.monotonic() - self._last_use < self.IDLE_STOP

    def _guarded(self):
        try:
            self.error = None
            self.run()
        except Exception as e:  # an input failing must never take lighting down
            self.error = str(e)
            log.warning("%s stopped: %s", type(self).__name__, e)
            time.sleep(3)        # avoid a tight restart loop

    def run(self):
        raise NotImplementedError


class ScreenFrame:
    def __init__(self, grid):
        self.grid = grid                      # (H, W, 3) floats 0..1

    def sample(self, pos01):
        h, w, _ = self.grid.shape
        xi = np.clip((pos01[:, 0] * (w - 1)).round().astype(int), 0, w - 1)
        yi = np.clip((pos01[:, 1] * (h - 1)).round().astype(int), 0, h - 1)
        return self.grid[yi, xi]


class ScreenSampler(_LazyWorker):
    """Grabs the primary display ~20 times a second and keeps a smoothed 48x27 colour grid."""

    GRID = (27, 48)
    FPS = 20
    SMOOTH = 0.45            # 0 = frozen, 1 = no smoothing

    def __init__(self):
        super().__init__()
        self.frame = None
        self.display = 1         # mss numbering: 1.. = each display, 0 = all of them as one picture

    def run(self):
        import mss
        gh, gw = self.GRID
        with mss.mss() as sct:
            shown = None
            while self._should_run():
                t0 = time.monotonic()
                if self.display != shown:                 # another display was picked
                    shown = self.display
                    mon = sct.monitors[shown] if 0 <= shown < len(sct.monitors) else sct.monitors[1]
                    self.frame = None
                shot = np.frombuffer(sct.grab(mon).bgra, np.uint8).reshape(mon["height"], mon["width"], 4)
                # Sample a sparse grid first (~4 px per cell side) instead of averaging every pixel of a 4K frame:
                # the full average cost ~100 ms per frame and starved the lighting thread.
                sy, sx = max(1, shot.shape[0] // (gh * 4)), max(1, shot.shape[1] // (gw * 4))
                sparse = shot[::sy, ::sx, 2::-1]
                h, w = (sparse.shape[0] // gh) * gh, (sparse.shape[1] // gw) * gw
                small = sparse[:h, :w].reshape(gh, h // gh, gw, w // gw, 3).mean(axis=(1, 3)) / 255.0
                prev = self.frame.grid if self.frame is not None else small
                self.frame = ScreenFrame(prev + (small - prev) * self.SMOOTH)
                time.sleep(max(0.0, 1 / self.FPS - (time.monotonic() - t0)))
        self.frame = None


class AudioFrame:
    def __init__(self, bands, level, bass, bands64=None, beat=False):
        self.bands, self.level, self.bass = bands, level, bass
        self.bands64, self.beat = bands64, beat          # what Studio effects get: 64 bands and a beat flag


class BeatDetector:
    """A beat is the bass jumping well above its recent average, at most every 0.25 s."""

    def __init__(self, ratio=1.45, floor=0.25, gap=0.25):
        self.ratio, self.floor, self.gap = ratio, floor, gap
        self.avg, self.last = 0.0, -1.0

    def update(self, bass, now):
        beat = bass > self.floor and bass > self.avg * self.ratio and now - self.last >= self.gap
        if beat:
            self.last = now
        self.avg = self.avg * 0.9 + bass * 0.1
        return beat


class AudioAnalyzer(_LazyWorker):
    """Listens to what's playing (WASAPI loopback of the default output) and keeps 16 log-spaced bands, 0..1, for the
    built-in effects, and 64 bands plus a beat flag for Studio effects."""

    RATE, BLOCK, BANDS, FINE = 48000, 1024, 16, 64
    DEVICE_CHECK = 3.0       # seconds between checks for a new default output (headphones plugged in)

    def __init__(self):
        super().__init__()
        self.frame = None
        edges = np.geomspace(40, 16000, self.BANDS + 1)
        freqs = np.fft.rfftfreq(self.BLOCK, 1 / self.RATE)
        self._bins = [(freqs >= lo) & (freqs < hi) for lo, hi in zip(edges, edges[1:])]
        # 64 bands from 30 Hz: the low ones are narrower than one FFT bin, so each takes its nearest bin
        fine = np.geomspace(30, 16000, self.FINE + 1)
        centres = np.sqrt(fine[:-1] * fine[1:])
        self._fine = [((freqs >= lo) & (freqs < hi)) if ((freqs >= lo) & (freqs < hi)).any()
                      else (np.arange(len(freqs)) == int(np.argmin(np.abs(freqs - c))))
                      for lo, hi, c in zip(fine, fine[1:], centres)]
        self._window = np.hanning(self.BLOCK)

    def run(self):
        import soundcard as sc
        warnings.filterwarnings("ignore", module="soundcard")        # "data discontinuity" noise
        while self._should_run():
            self._listen(sc, sc.default_speaker())
        self.frame = None

    def _listen(self, sc, speaker):
        """Analyse one output device until the effect stops or Windows switches the default output."""
        mic = sc.get_microphone(id=str(speaker.name), include_loopback=True)
        peak = np.full(self.BANDS, 1e-3)
        smooth = np.zeros(self.BANDS)
        fpeak = np.full(self.FINE, 1e-3)
        fsmooth = np.zeros(self.FINE)
        beats = BeatDetector()
        bass = 0.0
        checked = time.monotonic()
        with mic.recorder(samplerate=self.RATE, blocksize=self.BLOCK) as rec:
            while self._should_run():
                if time.monotonic() - checked > self.DEVICE_CHECK:
                    checked = time.monotonic()
                    if sc.default_speaker().id != speaker.id:
                        log.info("default output changed; listening to %s", sc.default_speaker().name)
                        return
                data = rec.record(numframes=self.BLOCK)
                mono = data.mean(axis=1) if data.ndim > 1 else data
                if len(mono) < self.BLOCK:
                    continue
                spec = np.abs(np.fft.rfft(mono[: self.BLOCK] * self._window))
                bands = np.array([spec[b].mean() if b.any() else 0.0 for b in self._bins])
                peak = np.maximum(bands, peak * 0.995)                   # slow auto-gain
                norm = np.clip(bands / np.maximum(peak, 1e-4), 0, 1)
                smooth = np.where(norm > smooth, norm, smooth * 0.82)     # fast attack, soft release
                b = float(norm[:3].mean())
                bass = b if b > bass else bass * 0.8
                fine = np.array([spec[m].mean() for m in self._fine])
                fpeak = np.maximum(fine, fpeak * 0.995)
                fnorm = np.clip(fine / np.maximum(fpeak, 1e-4), 0, 1)
                fsmooth = np.where(fnorm > fsmooth, fnorm, fsmooth * 0.82)
                self.frame = AudioFrame(smooth.copy(), float(smooth.mean()), bass, fsmooth.copy(),
                                        beats.update(b, time.monotonic()))
