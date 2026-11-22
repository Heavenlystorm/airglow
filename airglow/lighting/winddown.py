"""A volatile output ceiling. The lighting engine serializes commands and frame reads under its lock."""
import math
import secrets


class WindDown:
    def __init__(self):
        self.token = None
        self.session, self.version = secrets.token_hex(8), 0
        self.start_level = self.started = self.duration = 0

    def start(self, minutes, level, now):
        if type(minutes) is not int or not 5 <= minutes <= 120:
            raise ValueError("Choose a whole number from 5 to 120 minutes.")
        if self.token:
            raise ValueError("A wind-down timer is already running or holding the lights. Release it before starting another.")
        if level <= 0:
            raise ValueError("The current light level is zero. Start a fade when lighting is on.")
        self.start_level, self.started, self.duration = level, now, minutes * 60
        self.token = secrets.token_hex(12)
        self.version += 1

    def cancel(self, token):
        if not isinstance(token, str) or not token:
            raise ValueError("Choose the current timer to release it.")
        if self.token and token != self.token:
            raise ValueError("A different timer is active. Check its status before cancelling.")
        if self.token:
            self.token = None
            self.version += 1

    def _remaining(self, now):
        return max(0.0, self.duration - max(0.0, now - self.started))

    def cap(self, now):
        if self.token is None:
            return None
        return self.start_level * self._remaining(now) / self.duration

    def status(self, level, now):
        cap = self.cap(now)
        base = {"session": self.session, "version": self.version, "underlying": level}
        if cap is None:
            return {**base, "phase": "idle", "id": None}
        seconds = math.ceil(self._remaining(now))
        return {**base, "phase": "fading" if seconds else "held", "id": self.token, "remaining": seconds,
                "duration": self.duration, "initial": self.start_level, "cap": round(cap, 2)}
