#!/usr/bin/env python3
"""Write the synthetic six-month Abbott Lingo export and the check-ins logged alongside it.

The mental health page loads both as its sample. The trace is modelled, not copied: a seeded
random walk around a slowly improving daily baseline, meal responses with reactive dips, and a
dawn rise. It reproduces what a real Lingo export looks like to the charts: 5-minute readings
listed newest first, `201` for readings above the sensor's 200 mg/dL ceiling and `54` below its
55 floor, runs of `Out of range` rows at the ceiling, long ceiling plateaus, a gap at every
14-day sensor change, short signal dropouts, and one twelve-day break between sensors.

The check-ins are scored from the trace at the moment they are logged, so a low reads as shaky
and a ceiling plateau as foggy, and fortnightly PHQ-9 / GAD-7 scores ease over the six months.

    Usage: scripts/gen-sample-lingo.py crates/ui/data
"""

import math
import sys
from datetime import datetime, timedelta, timezone
from pathlib import Path

MASK = (1 << 64) - 1
SEED = 0x4C494E474F
TZ = timezone(timedelta(hours=-7))
# Last reading of the trace; the first is DAYS earlier.
END = datetime(2026, 11, 20, 3, 48, tzinfo=TZ)
DAYS = 183
STEP_MIN = 5
TRACE_FILE = "sample-lingo-6mo.csv"
CHECKINS_FILE = "sample-lingo-6mo-checkins.csv"
HEADER = "Time of Glucose Reading [T=(local time) +/- (time zone offset)], Measurement(mg/dL)"
CHECKINS_HEADER = "recorded_at,mood,anxiety,energy,sleep_hours,phq9,gad7,note"


class Rng:
    """SplitMix64, so the files are the same on every run."""

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


def sensor_gaps(rng):
    """(minutes after the first reading, length in minutes) the sensor sent nothing.

    A Lingo sensor lasts 14 days, and the wearer goes 11-19 hours before applying the next one.
    One break lasts twelve days, and a few short signal dropouts land mid-wear.
    """
    gaps = []
    for change in range(1, (DAYS - 2) // 14 + 1):
        start = (change * 14 * 1440) + int(rng.between(8, 20) * 60)
        gaps.append((start, int(rng.between(11, 19) * 60)))
    gaps.append((118 * 1440 + 20 * 60, 282 * 60 + 50))
    for _ in range(6):
        gaps.append((int(rng.between(1, DAYS - 2) * 1440), int(rng.between(35, 110))))
    return gaps


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


def trace(rng, start, gaps):
    """Modelled glucose every 5 minutes, (time, mg/dL before the sensor's caps), gaps left out."""
    eaten = meals(rng, start)
    # Months of slowly better control: the daily baseline drifts down about 20 mg/dL.
    baseline = [rng.between(150, 185) + 12 - 20 * d / DAYS for d in range(DAYS + 2)]
    noise = 0.0
    # Slow drift over hours, which holds the trace at the ceiling for long stretches.
    drift = 0.0
    rows = []
    meal_idx = 0
    for i in range(DAYS * 1440 // STEP_MIN):
        elapsed = i * STEP_MIN
        at = start + timedelta(minutes=elapsed)
        noise = 0.85 * noise + 2.2 * rng.normal()
        drift = 0.99 * drift + 4.0 * rng.normal()
        if any(s <= elapsed < s + length for s, length in gaps):
            continue
        clock = at.hour * 60 + at.minute
        day = elapsed / 1440
        g = baseline[int(day)] + 12 * math.sin(day * 0.9)
        g += 22 * math.exp(-(((clock - 330) / 75) ** 2))
        while meal_idx < len(eaten) and (at - eaten[meal_idx][0]).total_seconds() / 60 >= 420:
            meal_idx += 1
        for when, size in eaten[meal_idx : meal_idx + 6]:
            after = (at - when).total_seconds() / 60
            if -10 < after < 420:
                g += meal_rise(after, size)
                # A big meal overshoots into a reactive low about three hours later.
                if size > 130:
                    g -= 0.9 * size * math.exp(-(((after - 210) / 55) ** 2))
        rows.append((at, round(g + drift + noise)))
    return rows


def lingo_cells(rows, rng):
    """Render values the way a Lingo export does: capped at 201 / 54, with `Out of range` runs.

    The oldest reading always carries a number, since the timeline starts at it.
    """
    out = []
    out_of_range_left = 0
    for idx, (at, g) in enumerate(rows):
        if g > 200:
            if idx > 0 and out_of_range_left == 0 and rng.unit() < 0.012:
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


def clamp_score(value, lo, hi):
    return max(lo, min(hi, round(value)))


def checkins(rows, rng, start):
    """Check-ins every one to four days, scored from the glucose around the moment they're logged."""
    by_time = {at: g for at, g in rows}
    out = []
    day = 0.4
    next_scales = 0.0
    while day < DAYS - 0.1:
        at = start + timedelta(days=day)
        # Land on the trace's own 5-minute grid, so the readings before it can be looked up.
        at = at.replace(
            hour=int(rng.between(7, 23)),
            minute=(start.minute + 5 * int(rng.between(0, 11))) % 60,
            second=0,
            microsecond=0,
        )
        day += rng.between(1.0, 4.0)
        if at >= END:
            break
        progress = (at - start).total_seconds() / (DAYS * 86400)
        recent = [
            by_time[at - timedelta(minutes=m)]
            for m in range(0, 125, 5)
            if at - timedelta(minutes=m) in by_time
        ]
        now = recent[0] if recent else None
        mood = 4.6 + 2.4 * progress + 0.8 * rng.normal()
        anxiety = 6.4 - 2.6 * progress + 0.8 * rng.normal()
        energy = 4.8 + 1.8 * progress + 0.8 * rng.normal()
        if now is None:
            note = "Between sensors so no glucose to compare."
        elif now < 70:
            mood, anxiety, energy = mood - 2, anxiety + 3, energy - 1.5
            note = "Shaky and on edge as glucose dropped toward the sensor floor."
        elif now <= 140:
            mood, anxiety, energy = mood + 1, anxiety - 1, energy + 1
            note = "Steady and clear-headed while in range."
        elif max(recent) - min(recent) > 100:
            mood, anxiety = mood - 1, anxiety + 1.5
            note = "Irritable through a sharp swing after eating."
        elif all(g > 200 for g in recent):
            mood, anxiety, energy = mood - 1, anxiety + 1, energy - 2
            note = "Foggy and flat after hours pinned at the sensor ceiling."
        else:
            note = "Running high but holding steady."
        scales = ("", "")
        if day >= next_scales:
            next_scales = day + 14
            phq9 = clamp_score(13 - 7 * progress + 1.5 * rng.normal(), 0, 27)
            gad7 = clamp_score(12 - 6 * progress + 1.5 * rng.normal(), 0, 21)
            scales = (str(phq9), str(gad7))
        out.append(
            ",".join(
                [
                    at.strftime("%Y-%m-%d %H:%M"),
                    str(clamp_score(mood, 1, 10)),
                    str(clamp_score(anxiety, 1, 10)),
                    str(clamp_score(energy, 1, 10)),
                    f"{round(rng.between(5.5, 8.5) * 2) / 2:.1f}",
                    *scales,
                    note,
                ]
            )
        )
    return out


def main():
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    out_dir = Path(sys.argv[1])
    rng = Rng(SEED)
    start = END - timedelta(days=DAYS) + timedelta(minutes=STEP_MIN)
    rows = trace(rng, start, sensor_gaps(rng))
    cells = lingo_cells(rows, rng)
    lines = [HEADER] + [f"{at.strftime('%Y-%m-%dT%H:%M')}-07:00,{cell}" for at, cell in reversed(cells)]
    (out_dir / TRACE_FILE).write_text("\n".join(lines) + "\n")
    (out_dir / CHECKINS_FILE).write_text("\n".join([CHECKINS_HEADER] + checkins(rows, rng, start)) + "\n")


if __name__ == "__main__":
    main()
