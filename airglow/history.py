"""A rolling history for every sensor, so a tile added to the Overview shows the last ten minutes straight away."""

import collections
import threading


class SensorHistory:
    def __init__(self, size=300):
        self.size = size
        self.t = collections.deque(maxlen=size)
        self.series = {}
        self._misses = {}                # sensor id -> samples since it last reported (it is forgotten eventually)
        self._lock = threading.Lock()

    def add(self, t, values):
        with self._lock:
            self.t.append(round(t))
            for sid in [s for s in self.series if s not in values]:
                # a sensor that went away (an unplugged device, an LHM restart) must not keep its slot for the
                # life of the process: drop it once a whole window has gone by without a reading
                misses = self._misses.get(sid, 0) + 1
                if misses > self.size:
                    self.series.pop(sid, None)
                    self._misses.pop(sid, None)
                else:
                    self._misses[sid] = misses
            for sid in set(self.series) | set(values):
                q = self.series.get(sid)
                if q is None:            # first time seen: pad so every series lines up with the timestamps
                    q = self.series[sid] = collections.deque([None] * (len(self.t) - 1), maxlen=self.size)
                v = values.get(sid)
                q.append(None if v is None else round(v, 2))
                if sid in values:
                    self._misses[sid] = 0

    def get(self, ids):
        with self._lock:
            return {"t": list(self.t), "series": {i: list(self.series[i]) for i in ids if i in self.series}}
