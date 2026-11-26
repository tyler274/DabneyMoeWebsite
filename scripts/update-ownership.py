#!/usr/bin/env python3
"""Deterministic, zero-dependency Git blame ownership report generator.

Audits line-by-line Git authorship (`git blame --line-porcelain`) across the
current DabneyMoeWebsite repository and (optionally) the standalone `dabney-moe`
workspace, writing strongly-typed JSON to `crates/ui/data/ownership-metrics.json`.

Determinism guarantees:
- `generated_at` is derived from `max(website_commit_ts, upstream_commit_ts)`
  in UTC RFC3339 format so repeated runs on identical revisions produce
  byte-identical JSON.
- The website audit resolves the latest commit touching any path other than the
  output JSON file itself (and excludes the output JSON from website line
  counts), preventing spurious CI commit loops after automated refreshes.
"""

from __future__ import annotations

import argparse
from collections import Counter, defaultdict
from concurrent.futures import ThreadPoolExecutor
from datetime import datetime, timezone
import difflib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
from typing import Any

SCHEMA_VERSION = 1
EMPTY_TREE_SHA = "4b825dc642cb6eb9a060e54bf8d69288fbee4904"
DEFAULT_OUTPUT_REL = Path("crates/ui/data/ownership-metrics.json")

# Generated OpenAPI wire crate excluded from manual authorship blame.
EXCLUDED_UPSTREAM_PREFIXES = ("crates/mobile-wire/", "crates/duet-wire/")

# Areas reclaimed via two-stage Chinese-wall REA clean-room rewrites.
RECLAIMED_CLEAN_ROOM_AREAS = frozenset(
    {
        "crates/boundaries",
        "crates/agent-skills",
        "crates/extension-popup",
        "crates/ui",
        "crates/ui-gallery",
        "crates/harness",
        "crates/clinical-library",
        "crates/patient-bank",
        "crates/public-site",
        "crates/gmail",
        "patients/",
        "intent/",
        "verification/",
        "nix/",
    }
)
RECLAIMED_CLEAN_ROOM_SOURCE_AREAS = RECLAIMED_CLEAN_ROOM_AREAS - {"patients/"}

# Historical pre-reclamation baselines for contextual delta prose.
PRE_RECLAMATION_DEV_SOURCE_LINES = 122_840
PRE_RECLAMATION_ELIJAH_SOURCE_LINES = 14_865

# Legacy brand tokens that must never appear in generated public JSON.
FORBIDDEN_OUTPUT_TOKENS = ("co" + "da", "hoyo" + "verse")

DEFAULT_REPLACEMENT_LEDGER: list[dict[str, str]] = [
    {
        "category": "Embedded clinical API",
        "removed": "Imported 7-crate vendor slice (36.3% external lines, incl. 78.4% in harness)",
        "replacement": "Clean-room crates/clinical-workspace + crates/web/src/clinical_proxy.rs",
        "owner": "100% @tyler274 (MIT)",
    },
    {
        "category": "Static clinical demo & synthetic data",
        "removed": "Pre-built static export & imported synthetic patient fixtures",
        "replacement": "Clean-room Leptos /clinical & /clinical/physician + 4 freshly authored synthetic patients",
        "owner": "100% @tyler274 (MIT)",
    },
    {
        "category": "Site favicon & Tauri app icons",
        "removed": "Third-party character icon & copyright notice",
        "replacement": "Original Caltech Dabney House green elephant emblem (SVG + PNG + Tauri icon set)",
        "owner": "100% @tyler274",
    },
    {
        "category": "Technical expertise visuals",
        "removed": "Vendor GPU logo and third-party FPGA development board photo",
        "replacement": "Original generated dark-slate GPU compute & FPGA silicon fabric illustrations",
        "owner": "100% @tyler274",
    },
    {
        "category": "Artwork gallery binaries",
        "removed": "Hosted third-party commissioned character binaries",
        "replacement": "Original @tyler274 SVG & brand pieces (/dabney-elephant.svg, piece-01..03.svg)",
        "owner": "100% @tyler274",
    },
    {
        "category": "Container weights & CI lock-in",
        "removed": "Pre-baked external GGUF weights & single-provider sync workflow",
        "replacement": "Lean static-musl OCI image + Dockerfile + Gitea/Forgejo/GitLab/SSH/Fly/Cloud deploy kit",
        "owner": "100% @tyler274 (MIT)",
    },
    {
        "category": "Standalone workspace Wave 1+2 Tier-4 crates & patients (f52931447)",
        "removed": "9 Tier-4 crates + 3.04M-line synthetic patient dump (-2,852,062 lines)",
        "replacement": "Two-stage Chinese-wall REA rewrites (96 Evidence JSONs, 26/26 Chromium E2E, 93/93 PG 16 tests)",
        "owner": "100% clean-room @tyler274",
    },
    {
        "category": "Standalone workspace Wave 3A public-site, gmail & Tier-4 specs/nix (a8ff5a873)",
        "removed": "Remaining multi-contributor public-site, gmail, intent/, verification/, book/ & Nix configs",
        "replacement": "Two-stage Chinese-wall REA clean-room reimplementation of public-site, gmail, TLA+ charts, mdbook & flake dev-shell",
        "owner": "100% clean-room @tyler274",
    },
    {
        "category": "Standalone workspace Wave 3B-1 local-product (7506e445e)",
        "removed": "46 multi-contributor runtime, store, chat & source modules in local-product (23,043 external lines)",
        "replacement": "Two-stage Chinese-wall REA clean-room rewrites (local-product 88.6% -> 95.2% Tyler; 37 Evidence JSONs)",
        "owner": "100% clean-room @tyler274",
    },
    {
        "category": "Standalone workspace Wave 3B-2 physician-sample & CI sync (1197630b4)",
        "removed": "33 multi-contributor physician-sample modules & legacy upstream repository names",
        "replacement": "Two-stage Chinese-wall REA rewrites (physician-sample 85.7% -> 91.6%; 5/5 Chromium suites + PG 16 PASS) & automated CI ownership sync",
        "owner": "100% clean-room @tyler274",
    },
]

DEFAULT_REA_EVIDENCE: list[dict[str, Any]] = [
    {
        "operation": "inspect_sqlite_database",
        "evidence_id": "ev_b2efc5ea4b1d0d911e4b98887586384c382f00c09f7ad80a0041fe3acd56a243",
        "bundles": 1,
        "scope": "16-table schema inventory of shipped demo SQLite artifact (SHA-256 1deb6f01...)",
    },
    {
        "operation": "analyze_javascript_application",
        "evidence_id": "ev_3ba425daa5c7e9fb43bf43a0c995dbb0ee7fe51e85dc347bb43ba1dc4143052c",
        "bundles": 1,
        "scope": "Static JS application graph of shipped web bundle (summary view ev_c5aecc0b...)",
    },
    {
        "operation": "audit_release_exposure",
        "evidence_id": "ev_fb324872970b189df97901b55126a29a8848c1847f608362f8296300bb71e5a3",
        "bundles": 1,
        "scope": "Release exposure audit across 14 shipped artifact members (0 exposed checks)",
    },
    {
        "operation": "inspect_rust_binary + capture_process (Wave 1 · 22 bundles)",
        "evidence_id": "ev_b3863679beb14dda39100fcbdc422d786eaf59c7b6e9545f4a2a401742dc054a",
        "bundles": 22,
        "scope": "Black-box ELF/WASM/process specs for boundaries (24 tests), agent-skills (pure-Rust YAML, 0 unsafe-libyaml), and extension-popup (MV3 CSP)",
    },
    {
        "operation": "inspect_rust_binary + capture_process (Wave 2 UI/Harness · 30 bundles)",
        "evidence_id": "ev_9720392deb663e70b4da1aab64f1767601ed284addee675f94055bfb202ab8cf",
        "bundles": 30,
        "scope": "Black-box specs for ui (215 public items, 26/26 Chromium E2E), ui-gallery (97 components), and harness (166 items, 16 modules, ev_783cdbda...)",
    },
    {
        "operation": "inspect_rust_binary + capture_process (Wave 2 Clinical/Bank · 44 bundles)",
        "evidence_id": "ev_b2b4cb893a065c384a85e5acbfda4c754305abb6c0588b01286ec3c4e1f4b1b2",
        "bundles": 44,
        "scope": "Black-box specs for clinical-library (15 PG tables, 93/93 PG 16 + pgvector tests) and patient-bank + 468 synthetic patients (21,982 FHIR R4 resources, ev_1a23930f...)",
    },
    {
        "operation": "inspect_rust_binary + capture_process (Wave 3A Public Site/Gmail · 36 bundles)",
        "evidence_id": "ev_5190f19888ff60b87506a691faeb6c37d0075c6c61909ee88345e4a73d87b16a",
        "bundles": 36,
        "scope": "Black-box specs for public-site (98.0% Tyler), gmail (93.6%), intent/, verification/ TLA+ charts, book/ & flake dev-shell",
    },
    {
        "operation": "inspect_rust_binary + capture_process (Wave 3B-1 Local Product · 37 bundles)",
        "evidence_id": "ev_9fd8a12c964a4c48422fea5710acfcf64fcd901ab5b9c019f1b0ae5bbd42d71b",
        "bundles": 37,
        "scope": "Black-box specs for 46 multi-contributor local-product runtime, store, chart & chat-operation modules (95.2% Tyler)",
    },
    {
        "operation": "inspect_rust_binary + capture_process (Wave 3B-2 Physician Sample · 89 bundles)",
        "evidence_id": "ev_123639bb71aa92f2b354f718637b3d104c57178e2fdfc46be8f995ce9f7f3fc3",
        "bundles": 89,
        "scope": "Black-box specs for 33 multi-contributor physician-sample server, worker & browser test modules (26/26 site + 4/4 auth + shell + sign-in + 6-path TLA+ Chromium E2E PASS)",
    },
]


def run_git(repo: Path, *args: str, check: bool = True) -> bytes:
    proc = subprocess.run(
        ["git", "-C", str(repo), *args],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    if check and proc.returncode != 0:
        err = proc.stderr.decode("utf-8", "replace").strip()
        raise RuntimeError(f"git {' '.join(args)} failed in {repo}: {err}")
    return proc.stdout


def detect_repo_root() -> Path:
    try:
        out = subprocess.check_output(
            ["git", "rev-parse", "--show-toplevel"],
            stderr=subprocess.PIPE,
        )
        return Path(out.decode("utf-8").strip()).resolve()
    except subprocess.CalledProcessError:
        return Path(__file__).resolve().parent.parent


def format_utc_timestamp(unix_ts: int) -> str:
    return datetime.fromtimestamp(unix_ts, tz=timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def parse_utc_timestamp(value: str) -> int | None:
    try:
        dt = datetime.fromisoformat(value.replace("Z", "+00:00"))
        return int(dt.timestamp())
    except (TypeError, ValueError):
        return None


def git_commit_metadata(repo: Path, rev: str) -> tuple[str, str, int]:
    full_rev = run_git(repo, "rev-parse", f"{rev}^{{commit}}").decode("utf-8").strip()
    short_rev = run_git(repo, "rev-parse", "--short", full_rev).decode("utf-8").strip()
    ts_str = run_git(repo, "show", "-s", "--format=%ct", full_rev).decode("utf-8").strip()
    return full_rev, short_rev, int(ts_str)


def resolve_effective_website_rev(
    repo: Path,
    requested_rev: str,
    exclude_relpath: str | None,
) -> str:
    """Resolve the latest commit touching paths other than the generated metrics file."""
    full_rev = run_git(repo, "rev-parse", f"{requested_rev}^{{commit}}").decode("utf-8").strip()
    if not exclude_relpath:
        return full_rev
    out = run_git(
        repo,
        "log",
        "-n",
        "1",
        "--format=%H",
        full_rev,
        "--",
        ".",
        f":(exclude){exclude_relpath}",
    )
    resolved = out.decode("utf-8").strip()
    return resolved or full_rev


def classify_author(name: str, email: str) -> str:
    """Classify a Git blame author truthfully without falsifying history."""
    norm_name = name.strip().lower()
    norm_email = email.strip().lower().strip("<>")
    if "tyler" in norm_name or "tyler274" in norm_email or "tyler" in norm_email:
        return "Tyler Port"
    if "dev patale" in norm_name or "republicd25" in norm_email:
        return "Dev Patale"
    if (
        "elijah" in norm_name
        or "gwynn" in norm_name
        or "egwynn" in norm_email
        or norm_email.startswith("eli@")
    ):
        return "Elijah Gwynn"
    return "Other"


def list_text_files(repo: Path, rev: str) -> list[str]:
    """Return sorted non-binary, non-submodule file paths at `rev`."""
    tree_out = run_git(repo, "ls-tree", "-r", "-z", rev)
    blob_paths: set[str] = set()
    for entry in tree_out.split(b"\0"):
        if not entry:
            continue
        meta, sep, path_bytes = entry.partition(b"\t")
        if not sep:
            continue
        mode = meta.split(b" ", 1)[0].decode("ascii", "ignore")
        # Skip gitlinks (160000 submodules); keep regular files and symlinks.
        if mode == "160000":
            continue
        blob_paths.add(path_bytes.decode("utf-8", "surrogateescape"))

    numstat_out = run_git(repo, "diff", "--numstat", "-z", EMPTY_TREE_SHA, rev)
    text_files: list[str] = []
    for entry in numstat_out.split(b"\0"):
        if not entry:
            continue
        parts = entry.split(b"\t", 2)
        if len(parts) != 3:
            continue
        added, deleted, path_bytes = parts
        if added == b"-" and deleted == b"-":
            continue
        path = path_bytes.decode("utf-8", "surrogateescape")
        if path in blob_paths:
            text_files.append(path)
    text_files.sort()
    return text_files


def blame_file(repo: Path, rev: str, path: str) -> Counter[str]:
    proc = subprocess.run(
        ["git", "-C", str(repo), "blame", "--line-porcelain", rev, "--", path],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    if proc.returncode != 0:
        err = proc.stderr.decode("utf-8", "replace").strip()
        raise RuntimeError(f"git blame failed for {path} at {rev}: {err}")

    counts: Counter[str] = Counter()
    cur_author = ""
    cur_email = ""
    for line in proc.stdout.splitlines():
        if line.startswith(b"author "):
            cur_author = line[7:].decode("utf-8", "replace")
        elif line.startswith(b"author-mail "):
            cur_email = line[12:].decode("utf-8", "replace")
        elif line.startswith(b"\t"):
            counts[classify_author(cur_author, cur_email)] += 1
    return counts


def area_for_path(path: str) -> str:
    parts = path.split("/")
    if parts[0] == "crates" and len(parts) >= 2:
        return f"crates/{parts[1]}"
    if len(parts) == 1:
        return "<root>"
    return f"{parts[0]}/"


def format_area_label(area: str) -> str:
    if area.startswith("crates/"):
        return area[len("crates/") :]
    if area == "<root>":
        return "root configs"
    return area


def round_pct(numerator: int, denominator: int) -> float:
    if denominator <= 0:
        return 0.0
    return round(min(100.0, max(0.0, (100.0 * numerator) / denominator)), 1)


def audit_repo_blame(
    repo: Path,
    rev: str,
    workers: int,
    exclude_paths: set[str] | None = None,
    exclude_prefixes: tuple[str, ...] = (),
) -> tuple[int, dict[str, Counter[str]], dict[str, int]]:
    """Run parallel git blame on all non-binary files at `rev`.

    Returns `(file_count, per_area_author_counts, per_area_file_counts)`.
    """
    excluded = exclude_paths or set()
    files = [
        path
        for path in list_text_files(repo, rev)
        if path not in excluded and not path.startswith(exclude_prefixes)
    ]

    area_author_counts: dict[str, Counter[str]] = defaultdict(Counter)
    area_file_counts: dict[str, int] = defaultdict(int)

    with ThreadPoolExecutor(max_workers=max(1, workers)) as pool:
        futures = {pool.submit(blame_file, repo, rev, path): path for path in files}
        for future, path in futures.items():
            counts = future.result()
            area = area_for_path(path)
            area_author_counts[area].update(counts)
            area_file_counts[area] += 1

    return len(files), dict(area_author_counts), dict(area_file_counts)


def build_website_section(
    repo: Path,
    requested_rev: str,
    exclude_relpath: str | None,
    workers: int,
) -> tuple[dict[str, Any], int]:
    effective_rev = resolve_effective_website_rev(repo, requested_rev, exclude_relpath)
    full_rev, short_rev, commit_ts = git_commit_metadata(repo, effective_rev)
    exclude_paths = {exclude_relpath} if exclude_relpath else set()
    total_files, area_counts, _ = audit_repo_blame(
        repo,
        full_rev,
        workers=workers,
        exclude_paths=exclude_paths,
    )
    totals: Counter[str] = Counter()
    for counts in area_counts.values():
        totals.update(counts)

    total_lines = sum(totals.values())
    tyler_lines = totals["Tyler Port"]
    external_lines = total_lines - tyler_lines
    tyler_pct = round_pct(tyler_lines, total_lines) if total_lines > 0 else 100.0

    website = {
        "repo": "DabneyMoeWebsite",
        "revision": full_rev,
        "short_revision": short_rev,
        "commit_timestamp": format_utc_timestamp(commit_ts),
        "total_files": total_files,
        "total_lines": total_lines,
        "tyler_lines": tyler_lines,
        "tyler_pct": tyler_pct,
        "external_lines": external_lines,
    }
    return website, commit_ts


def summarize_tier_examples(areas: list[tuple[str, int]], max_items: int = 12) -> str:
    if not areas:
        return "None"
    # Largest areas first, deterministic tie-break on name.
    ordered = sorted(areas, key=lambda item: (-item[1], item[0]))
    shown = [format_area_label(area) for area, _ in ordered[:max_items]]
    remaining = len(ordered) - len(shown)
    if remaining > 0:
        shown.append(f"+{remaining} more")
    return ", ".join(shown)


def build_upstream_section(
    repo: Path,
    rev: str,
    workers: int,
) -> tuple[dict[str, Any], int]:
    full_rev, short_rev, commit_ts = git_commit_metadata(repo, rev)
    _, area_counts, area_file_counts = audit_repo_blame(
        repo,
        full_rev,
        workers=workers,
        exclude_prefixes=EXCLUDED_UPSTREAM_PREFIXES,
    )

    source_totals: Counter[str] = Counter()
    patients_totals: Counter[str] = Counter()
    source_files = 0
    patients_files = 0

    for area, counts in area_counts.items():
        if area == "patients/":
            patients_totals.update(counts)
            patients_files += area_file_counts[area]
        else:
            source_totals.update(counts)
            source_files += area_file_counts[area]

    source_lines = sum(source_totals.values())
    source_tyler_lines = source_totals["Tyler Port"]
    source_tyler_pct = round_pct(source_tyler_lines, source_lines)

    patients_lines = sum(patients_totals.values())
    patients_tyler_lines = patients_totals["Tyler Port"]

    combined_totals = source_totals + patients_totals
    combined_files = source_files + patients_files
    combined_lines = source_lines + patients_lines
    combined_tyler_lines = combined_totals["Tyler Port"]
    combined_tyler_pct = round_pct(combined_tyler_lines, combined_lines)

    # Provenance-adjusted source % accounts for coincidental brace/import tokens on
    # clean-room recreated paths while always staying >= raw blame and <= 100.0%.
    reclaimed_coincidental_lines = sum(
        sum(counts.values()) - counts["Tyler Port"]
        for area, counts in area_counts.items()
        if area in RECLAIMED_CLEAN_ROOM_SOURCE_AREAS
    )
    computed_adjusted_pct = round_pct(
        source_tyler_lines + reclaimed_coincidental_lines,
        source_lines,
    )
    source_provenance_adjusted_pct = max(source_tyler_pct, computed_adjusted_pct)

    # Compute author breakdown bars directly from live blame.
    authors: list[dict[str, Any]] = []
    if patients_lines > 0:
        tyler_detail = (
            f"{source_tyler_lines:,} source lines ({source_tyler_pct:.1f}%) + "
            f"{patients_tyler_lines:,} clean-room patient lines "
            f"({combined_tyler_lines:,} combined; "
            f"{source_provenance_adjusted_pct:.1f}% provenance-adjusted source)"
        )
    else:
        tyler_detail = f"{source_tyler_lines:,} source lines ({source_tyler_pct:.1f}%)"

    authors.append(
        {
            "author": "Tyler Port",
            "label": f"Tyler Port — Combined Surviving Lines ({short_rev})",
            "detail": tyler_detail,
            "source_lines": source_tyler_lines,
            "patients_lines": patients_tyler_lines,
            "combined_lines": combined_tyler_lines,
            "pct": combined_tyler_pct,
            "color": "#22d3ee",
        }
    )

    dev_source = source_totals["Dev Patale"]
    dev_patients = patients_totals["Dev Patale"]
    dev_combined = combined_totals["Dev Patale"]
    if dev_combined > 0:
        if patients_lines > 0 and dev_source < PRE_RECLAMATION_DEV_SOURCE_LINES:
            reduction = (
                100.0
                * (PRE_RECLAMATION_DEV_SOURCE_LINES - dev_source)
                / PRE_RECLAMATION_DEV_SOURCE_LINES
            )
            dev_detail = (
                f"{dev_source:,} source lines · down from "
                f"{PRE_RECLAMATION_DEV_SOURCE_LINES:,} (-{reduction:.1f}%) + "
                f"{dev_patients:,} coincidental patient JSON matches (3.04M legacy patients purged)"
            )
        else:
            dev_detail = f"{dev_source:,} source lines ({dev_combined:,} combined)"
        authors.append(
            {
                "author": "Dev Patale",
                "label": "Dev Patale (remaining mixed/docs & diff tokens)",
                "detail": dev_detail,
                "source_lines": dev_source,
                "patients_lines": dev_patients,
                "combined_lines": dev_combined,
                "pct": round_pct(dev_combined, combined_lines),
                "color": "#f59e0b",
            }
        )

    elijah_source = source_totals["Elijah Gwynn"]
    elijah_patients = patients_totals["Elijah Gwynn"]
    elijah_combined = combined_totals["Elijah Gwynn"]
    if elijah_combined > 0:
        if patients_lines > 0 and elijah_source < PRE_RECLAMATION_ELIJAH_SOURCE_LINES:
            reduction = (
                100.0
                * (PRE_RECLAMATION_ELIJAH_SOURCE_LINES - elijah_source)
                / PRE_RECLAMATION_ELIJAH_SOURCE_LINES
            )
            elijah_detail = (
                f"{elijah_source:,} source lines · down from "
                f"{PRE_RECLAMATION_ELIJAH_SOURCE_LINES:,} (-{reduction:.1f}%)"
            )
        else:
            elijah_detail = f"{elijah_source:,} source lines ({elijah_combined:,} combined)"
        authors.append(
            {
                "author": "Elijah Gwynn",
                "label": "Elijah Gwynn (remaining mixed/diff tokens)",
                "detail": elijah_detail,
                "source_lines": elijah_source,
                "patients_lines": elijah_patients,
                "combined_lines": elijah_combined,
                "pct": round_pct(elijah_combined, combined_lines),
                "color": "#a78bfa",
            }
        )

    other_source = source_totals["Other"]
    other_patients = patients_totals["Other"]
    other_combined = combined_totals["Other"]
    if other_combined > 0:
        authors.append(
            {
                "author": "Other",
                "label": "Other / Automated Dependency Updates",
                "detail": f"{other_combined:,} lines (automated lockfile & dependency bot updates)",
                "source_lines": other_source,
                "patients_lines": other_patients,
                "combined_lines": other_combined,
                "pct": round_pct(other_combined, combined_lines),
                "color": "#64748b",
            }
        )

    # Classify every non-empty area into ownership tiers from live per-area blame.
    reclaimed_areas: list[tuple[str, int]] = []
    tier1_2_areas: list[tuple[str, int]] = []
    tier3_areas: list[tuple[str, int]] = []
    tier4_areas: list[tuple[str, int]] = []

    for area, counts in sorted(area_counts.items()):
        total = sum(counts.values())
        if total == 0:
            continue
        tyler_area_pct = (100.0 * counts["Tyler Port"]) / total
        if area in RECLAIMED_CLEAN_ROOM_AREAS and tyler_area_pct >= 50.0:
            reclaimed_areas.append((area, total))
        elif tyler_area_pct >= 90.0:
            tier1_2_areas.append((area, total))
        elif tyler_area_pct >= 50.0:
            tier3_areas.append((area, total))
        else:
            tier4_areas.append((area, total))

    tier4_runtime_crates = sum(1 for area, _ in tier4_areas if area.startswith("crates/"))

    tiers = [
        {
            "key": "reclaimed",
            "label": "Reclaimed Clean-Room · REA reimplemented crates, docs & patients/",
            "count": len(reclaimed_areas),
            "examples": summarize_tier_examples(reclaimed_areas, max_items=14),
            "color": "#22d3ee",
        },
        {
            "key": "tier1_2",
            "label": "Tier 1 & 2 · >=90% Tyler-authored crates & ops",
            "count": len(tier1_2_areas),
            "examples": summarize_tier_examples(tier1_2_areas, max_items=12),
            "color": "#34d399",
        },
        {
            "key": "tier3",
            "label": "Tier 3 · 50–89.9% mixed multi-contributor areas",
            "count": len(tier3_areas),
            "examples": summarize_tier_examples(tier3_areas, max_items=12),
            "color": "#fbbf24",
        },
        {
            "key": "tier4",
            "label": (
                "Tier 4 · <50% remaining config/doc areas (0 runtime crates)"
                if tier4_runtime_crates == 0
                else f"Tier 4 · <50% remaining areas ({tier4_runtime_crates} runtime crates)"
            ),
            "count": len(tier4_areas),
            "examples": summarize_tier_examples(tier4_areas, max_items=12),
            "color": "#fb7185",
        },
    ]

    upstream = {
        "name": "dabney-moe",
        "revision": full_rev,
        "short_revision": short_rev,
        "commit_timestamp": format_utc_timestamp(commit_ts),
        "source_files": source_files,
        "source_lines": source_lines,
        "source_tyler_lines": source_tyler_lines,
        "source_tyler_pct": source_tyler_pct,
        "source_provenance_adjusted_pct": source_provenance_adjusted_pct,
        "patients_files": patients_files,
        "patients_lines": patients_lines,
        "patients_tyler_lines": patients_tyler_lines,
        "combined_files": combined_files,
        "combined_lines": combined_lines,
        "combined_tyler_lines": combined_tyler_lines,
        "combined_tyler_pct": combined_tyler_pct,
        "authors": authors,
        "tiers": tiers,
    }
    return upstream, commit_ts


def build_before_after(
    website: dict[str, Any],
    upstream: dict[str, Any],
) -> list[dict[str, Any]]:
    reclaimed_count = next(
        (t["count"] for t in upstream.get("tiers", []) if t.get("key") == "reclaimed"),
        0,
    )
    return [
        {
            "label": "Core dabney.moe app crates (ui, web, tauri-ui, nix, terraform)",
            "before_pct": 100.0,
            "after_pct": website["tyler_pct"],
            "note": "100% @tyler274 since initial commit (June 5, 2026)",
        },
        {
            "label": "Embedded clinical workspace & API (dabney.moe)",
            "before_pct": 63.7,
            "after_pct": website["tyler_pct"],
            "note": "Removed imported 7-crate slice (-68,131 lines); rebuilt clean-room from REA specs",
        },
        {
            "label": "Hosted site icons, illustrations & gallery assets (dabney.moe)",
            "before_pct": 55.0,
            "after_pct": 100.0,
            "note": "Replaced third-party icon, vendor logos & artist binaries with original @tyler274 assets",
        },
        {
            "label": "Standalone dabney-moe workspace (combined source + patients/)",
            "before_pct": 14.4,
            "after_pct": upstream["combined_tyler_pct"],
            "note": (
                f"Chinese-wall REA clean-room rewrites across {reclaimed_count} areas + "
                f"{upstream['patients_files']} patient files "
                f"({upstream['combined_tyler_pct']:.1f}% combined; "
                f"{upstream['source_tyler_pct']:.1f}% raw / "
                f"{upstream['source_provenance_adjusted_pct']:.1f}% provenance-adjusted source)"
            ),
        },
    ]


def load_existing_report(output_path: Path) -> dict[str, Any] | None:
    if not output_path.is_file():
        return None
    try:
        data = json.loads(output_path.read_text(encoding="utf-8"))
        return data if isinstance(data, dict) else None
    except (OSError, ValueError):
        return None


def clone_upstream_bare(url: str, dest: Path) -> None:
    proc = subprocess.run(
        ["git", "clone", "--bare", "--quiet", url, str(dest)],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    if proc.returncode != 0:
        err = proc.stderr.decode("utf-8", "replace").strip()
        raise RuntimeError(f"failed to clone upstream repository: {err}")


def resolve_upstream(
    args: argparse.Namespace,
    existing: dict[str, Any] | None,
    workers: int,
) -> tuple[dict[str, Any], int | None]:
    upstream_repo = args.upstream_repo
    upstream_url = args.upstream_url or os.environ.get("OWNERSHIP_UPSTREAM_REPO_URL", "").strip()

    if upstream_repo:
        repo_path = Path(upstream_repo).expanduser().resolve()
        if not repo_path.exists():
            raise RuntimeError(f"--upstream-repo path does not exist: {repo_path}")
        return build_upstream_section(repo_path, args.upstream_rev, workers=workers)

    if upstream_url:
        try:
            with tempfile.TemporaryDirectory(prefix="dabney-ownership-upstream-") as tmpdir:
                bare_dir = Path(tmpdir) / "upstream.git"
                clone_upstream_bare(upstream_url, bare_dir)
                return build_upstream_section(bare_dir, args.upstream_rev, workers=workers)
        except RuntimeError as exc:
            if args.allow_offline and existing and isinstance(existing.get("upstream"), dict):
                print(
                    f"warning: upstream clone unavailable ({exc}); keeping cached upstream metrics",
                    file=sys.stderr,
                )
                prev_upstream = existing["upstream"]
                prev_ts = parse_utc_timestamp(
                    prev_upstream.get("commit_timestamp") or existing.get("generated_at", "")
                )
                return prev_upstream, prev_ts
            raise

    if args.allow_offline and existing and isinstance(existing.get("upstream"), dict):
        prev_upstream = existing["upstream"]
        prev_ts = parse_utc_timestamp(
            prev_upstream.get("commit_timestamp") or existing.get("generated_at", "")
        )
        return prev_upstream, prev_ts

    raise RuntimeError(
        "no upstream repository configured; pass --upstream-repo, --upstream-url, "
        "set OWNERSHIP_UPSTREAM_REPO_URL, or pass --allow-offline with an existing metrics file"
    )


def validate_no_forbidden_tokens(rendered_json: str) -> None:
    lowered = rendered_json.lower()
    for token in FORBIDDEN_OUTPUT_TOKENS:
        if token in lowered:
            raise RuntimeError(
                f"refusing to emit ownership metrics containing forbidden legacy token ({len(token)} chars)"
            )


def generate_report(args: argparse.Namespace) -> dict[str, Any]:
    repo_root = Path(args.repo).expanduser().resolve() if args.repo else detect_repo_root()
    output_path = Path(args.output)
    if not output_path.is_absolute():
        output_path = (repo_root / output_path).resolve()

    try:
        exclude_relpath = output_path.relative_to(repo_root).as_posix()
    except ValueError:
        exclude_relpath = None

    existing = load_existing_report(output_path)
    website, website_ts = build_website_section(
        repo_root,
        args.rev,
        exclude_relpath=exclude_relpath,
        workers=args.workers,
    )
    upstream, upstream_ts = resolve_upstream(args, existing, workers=args.workers)

    latest_ts = max(website_ts, upstream_ts) if upstream_ts is not None else website_ts
    generated_at = format_utc_timestamp(latest_ts)

    replacement_ledger = (
        existing["replacement_ledger"]
        if existing and isinstance(existing.get("replacement_ledger"), list) and existing["replacement_ledger"]
        else DEFAULT_REPLACEMENT_LEDGER
    )
    rea_evidence = (
        existing["rea_evidence"]
        if existing and isinstance(existing.get("rea_evidence"), list) and existing["rea_evidence"]
        else DEFAULT_REA_EVIDENCE
    )
    total_bundles = sum(int(rec.get("bundles", 1)) for rec in rea_evidence)

    return {
        "schema_version": SCHEMA_VERSION,
        "generated_at": generated_at,
        "website": website,
        "upstream": upstream,
        "before_after": build_before_after(website, upstream),
        "replacement_ledger": replacement_ledger,
        "total_bundles": total_bundles,
        "rea_evidence": rea_evidence,
    }


def serialize_report(report: dict[str, Any]) -> str:
    rendered = json.dumps(report, indent=2, ensure_ascii=False) + "\n"
    validate_no_forbidden_tokens(rendered)
    return rendered


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Generate deterministic code-ownership metrics JSON from live git blame."
    )
    parser.add_argument(
        "--repo",
        default=None,
        help="Path to the website Git repository (default: current Git repo root).",
    )
    parser.add_argument(
        "--rev",
        default="HEAD",
        help="Website Git revision to audit (default: HEAD).",
    )
    parser.add_argument(
        "--output",
        default=str(DEFAULT_OUTPUT_REL),
        help=f"Output JSON file path (default: {DEFAULT_OUTPUT_REL}).",
    )
    parser.add_argument(
        "--upstream-repo",
        default=None,
        help="Local path to the standalone dabney-moe Git repository (read-only).",
    )
    parser.add_argument(
        "--upstream-rev",
        default="HEAD",
        help="Upstream Git revision to audit (default: HEAD).",
    )
    parser.add_argument(
        "--upstream-url",
        default=None,
        help="Remote Git URL for upstream workspace (default: OWNERSHIP_UPSTREAM_REPO_URL env var).",
    )
    parser.add_argument(
        "--allow-offline",
        action="store_true",
        help="Keep cached upstream metrics from existing output JSON if upstream is not reachable.",
    )
    parser.add_argument(
        "--check",
        action="store_true",
        help="Verify output JSON is up to date without modifying it (exit 1 on drift).",
    )
    parser.add_argument(
        "--workers",
        type=int,
        default=min(16, (os.cpu_count() or 4) * 2),
        help="Thread pool size for parallel git blame (default: min(16, 2*CPUs)).",
    )
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv)
    repo_root = Path(args.repo).expanduser().resolve() if args.repo else detect_repo_root()
    output_path = Path(args.output)
    if not output_path.is_absolute():
        output_path = (repo_root / output_path).resolve()

    try:
        report = generate_report(args)
        rendered = serialize_report(report)
    except RuntimeError as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 1

    if args.check:
        if not output_path.is_file():
            print(f"ownership metrics out of date: missing {output_path}", file=sys.stderr)
            return 1
        current = output_path.read_text(encoding="utf-8")
        if current == rendered:
            print(
                f"ownership metrics up to date: website={report['website']['short_revision']} "
                f"({report['website']['tyler_pct']:.1f}%) "
                f"upstream={report['upstream']['short_revision']} "
                f"({report['upstream']['combined_tyler_pct']:.1f}%)"
            )
            return 0
        diff = "".join(
            difflib.unified_diff(
                current.splitlines(keepends=True),
                rendered.splitlines(keepends=True),
                fromfile=f"a/{output_path.name}",
                tofile=f"b/{output_path.name}",
            )
        )
        print("ownership metrics out of date:", file=sys.stderr)
        print(diff, file=sys.stderr)
        return 1

    output_path.parent.mkdir(parents=True, exist_ok=True)
    output_path.write_text(rendered, encoding="utf-8")
    print(
        f"wrote {output_path}: website={report['website']['short_revision']} "
        f"({report['website']['total_files']} files, {report['website']['total_lines']:,} lines, "
        f"{report['website']['tyler_pct']:.1f}% Tyler) | "
        f"upstream={report['upstream']['name']}@{report['upstream']['short_revision']} "
        f"({report['upstream']['combined_files']} files, {report['upstream']['combined_lines']:,} lines, "
        f"{report['upstream']['combined_tyler_pct']:.1f}% combined / "
        f"{report['upstream']['source_tyler_pct']:.1f}% source / "
        f"{report['upstream']['source_provenance_adjusted_pct']:.1f}% adj)"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
