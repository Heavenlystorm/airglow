"""Fan curve maths: interpolation, temperature source, ramp limiting and the safety override."""

from .config import SAFETY_TEMP


def curve_duty(points, temp):
    """Linear interpolation over [temp, duty] points; flat before the first and after the last."""
    points = sorted(points)
    if temp <= points[0][0]:
        return float(points[0][1])
    for (t0, d0), (t1, d1) in zip(points, points[1:]):
        if temp <= t1:
            return d0 + (d1 - d0) * (temp - t0) / (t1 - t0)
    return float(points[-1][1])


def source_temp(sensors, source):
    cpu, gpu = sensors.get("cpu_temp"), sensors.get("gpu_temp")
    if source == "cpu":
        return cpu
    if source == "gpu":
        return gpu
    temps = [t for t in (cpu, gpu) if t is not None]
    return max(temps) if temps else None


def lowest_duty(points):
    """The curve's speed at its coolest point."""
    return float(min(points)[1])


def target_duty(points, temp, failsafe):
    """What the fans should aim for. No temperature -> failsafe; very hot -> 100%."""
    if temp is None:
        return failsafe
    if temp >= SAFETY_TEMP:
        return 100
    return curve_duty(points, temp)


def step_toward(current, target, max_step):
    """Move at most max_step toward target per tick (but always jump straight up to 100)."""
    if current is None or target >= 100:
        return round(target)
    delta = max(-max_step, min(max_step, target - current))
    return round(current + delta)
