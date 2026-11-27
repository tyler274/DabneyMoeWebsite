#!/usr/bin/env python3
"""Write the synthetic 14-day Abbott Lingo export the mental health page loads as its sample.

The trace is modelled, not copied: a seeded random walk around a daily baseline, meal
responses with reactive dips, and a dawn rise. It reproduces what a real Lingo export looks
like to the charts: 5-minute readings listed newest first, `201` for readings above the
sensor's 200 mg/dL ceiling and `54` below its 55 floor, runs of `Out of range` rows at the
ceiling, long ceiling plateaus, and one overnight sensor gap.

Usage: scripts/gen-sample-lingo.py > crates/ui/data/sample-lingo-14d.csv
"""

import math
import sys
from datetime import datetime, timedelta, timezone

MASK = (1 << 64) - 1
SEED = 0x4C494E474F
TZ = timezone(timedelta(hours=-7))
# Last reading of the trace; the first is 14 days earlier.
END = datetime(2026, 11, 20, 3, 48, tzinfo=TZ)
DAYS = 14
STEP_MIN = 5
# (minutes after the first reading, length in minutes) the sensor sent nothing.
GAPS = [(4 * 1440 + 60, 19 * 60 + 20), (8 * 1440 + 600, 70)]
HEADER = "Time of Glucose Reading [T=(local time) +/- (time zone offset)], Measurement(mg/dL)"


class Rng:
    """SplitMix64, so the file is the same on every run."""

    def __init__(self, seed):
        self.state = seed & MASK

    def unit(self):
        self.state = (self.state + 0x9E3779B97F4A7C15) & MASK
        z = self.state
        z = ((z ^ (z >> 30)) * 0xBF58476D1CE4E5B9) & MASK
        z = ((z ^ (z >> 27)) * 0x94D049BB133111EB) & MASK
        return ((z ^ (z >> 31)) >> 11) / float(1 << 53)

    def between(self, lo, hi):
        return lo + (hi - lo) * self.unit()

    def normal(self):
        u = max(self.unit(), 1e-12)
        return math.sqrt(-2 * math.log(u)) * math.cos(2 * math.pi * self.unit())


def meal_rise(minutes_after, size):
    """Gamma-shaped rise peaking about an hour after eating."""
    if minutes_after <= 0:
        return 0.0
    x = minutes_after / 60
    return size * x * math.exp(1 - x)


def meals(rng, start):
    out = []
    for day in range(-1, DAYS + 1):
        midnight = datetime(start.year, start.month, start.day, tzinfo=TZ) + timedelta(days=day)
        for hour, spread, lo, hi in [(8.0, 0.7, 60, 110), (13.0, 0.8, 80, 150), (19.0, 1.0, 90, 160)]:
            out.append((midnight + timedelta(hours=hour + rng.between(-spread, spread)), rng.between(lo, hi)))
        if rng.unit() < 0.5:
            out.append((midnight + timedelta(hours=22.5 + rng.between(-0.6, 0.6)), rng.between(50, 100)))
    return out


def trace():
    rng = Rng(SEED)
    start = END - timedelta(days=DAYS) + timedelta(minutes=STEP_MIN)
    eaten = meals(rng, start)
    baseline = [rng.between(150, 185) for _ in range(DAYS + 2)]
    noise = 0.0
    # Slow drift over hours, which holds the trace at the ceiling for long stretches.
    drift = 0.0
    rows = []
    for i in range(DAYS * 1440 // STEP_MIN):
        elapsed = i * STEP_MIN
        at = start + timedelta(minutes=elapsed)
        noise = 0.85 * noise + 2.2 * rng.normal()
        drift = 0.99 * drift + 4.0 * rng.normal()
        if any(s <= elapsed < s + length for s, length in GAPS):
            continue
        clock = at.hour * 60 + at.minute
        day = elapsed / 1440
        g = baseline[int(day)] + 12 * math.sin(day * 0.9)
        g += 22 * math.exp(-(((clock - 330) / 75) ** 2))
        for when, size in eaten:
            after = (at - when).total_seconds() / 60
            if -10 < after < 420:
                g += meal_rise(after, size)
                # A big meal overshoots into a reactive low about three hours later.
                if size > 130:
                    g -= 0.9 * size * math.exp(-(((after - 210) / 55) ** 2))
        rows.append((at, round(g + drift + noise)))
    return rows, rng


def lingo_cells(rows, rng):
    """Render values the way a Lingo export does: capped at 201 / 54, with `Out of range` runs."""
    out = []
    out_of_range_left = 0
    for at, g in rows:
        if g > 200:
            if out_of_range_left == 0 and rng.unit() < 0.012:
                out_of_range_left = 1 + int(rng.unit() * 6)
            if out_of_range_left:
                out_of_range_left -= 1
                out.append((at, "Out of range"))
            else:
                out.append((at, "201"))
        else:
            out_of_range_left = 0
            out.append((at, "54" if g < 55 else str(g)))
    return out


def main():
    rows, rng = trace()
    cells = lingo_cells(rows, rng)
    lines = [HEADER]
    for at, cell in reversed(cells):
        lines.append(f"{at.strftime('%Y-%m-%dT%H:%M')}-07:00,{cell}")
    sys.stdout.write("\n".join(lines) + "\n")


if __name__ == "__main__":
    main()
