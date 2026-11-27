#!/usr/bin/env python3
"""Unit tests for scripts/update-ownership.py."""

from __future__ import annotations

import importlib.util
import io
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile
import unittest
from unittest import mock

SCRIPT_PATH = Path(__file__).resolve().parent / "update-ownership.py"
REPO_ROOT = SCRIPT_PATH.parent.parent


def load_module():
    spec = importlib.util.spec_from_file_location("update_ownership", SCRIPT_PATH)
    assert spec is not None and spec.loader is not None
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


update_ownership = load_module()


def git_run(repo: Path, *args: str, env: dict[str, str] | None = None) -> str:
    full_env = os.environ.copy()
    if env:
        full_env.update(env)
    proc = subprocess.run(
        ["git", "-C", str(repo), *args],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        env=full_env,
        check=True,
    )
    return proc.stdout.decode("utf-8").strip()


def commit_as(
    repo: Path,
    name: str,
    email: str,
    timestamp: str,
    message: str,
) -> str:
    env = {
        "GIT_AUTHOR_NAME": name,
        "GIT_AUTHOR_EMAIL": email,
        "GIT_COMMITTER_NAME": name,
        "GIT_COMMITTER_EMAIL": email,
        "GIT_AUTHOR_DATE": timestamp,
        "GIT_COMMITTER_DATE": timestamp,
    }
    git_run(repo, "add", "-A", env=env)
    git_run(repo, "commit", "-m", message, env=env)
    return git_run(repo, "rev-parse", "HEAD")


def init_repo(path: Path) -> None:
    path.mkdir(parents=True, exist_ok=True)
    git_run(path, "init", "-b", "main")


class UpdateOwnershipTests(unittest.TestCase):
    def _create_synthetic_repos(self, root: Path) -> tuple[Path, Path]:
        website = root / "website"
        upstream = root / "upstream"
        init_repo(website)
        init_repo(upstream)

        # Website repo: 100% Tyler Port + binary file + empty file.
        (website / "crates" / "ui" / "src").mkdir(parents=True)
        (website / "crates" / "ui" / "src" / "lib.rs").write_text(
            "pub fn hello() -> &'static str {\n    \"dabney.moe\"\n}\n",
            encoding="utf-8",
        )
        (website / "public").mkdir(parents=True)
        (website / "public" / "logo.png").write_bytes(
            b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR\x00\x00\x00\x01"
        )
        (website / "empty.txt").write_text("", encoding="utf-8")
        commit_as(
            website,
            "tyler",
            "4147075+tyler274@users.noreply.github.com",
            "2026-11-20T12:00:00Z",
            "Initial website commit",
        )

        # Upstream repo: multi-author files across tiers + patients/ + excluded mobile-wire/.
        (upstream / "crates" / "alpha" / "src").mkdir(parents=True)
        (upstream / "crates" / "alpha" / "src" / "lib.rs").write_text(
            "".join(f"pub const A{i}: usize = {i};\n" for i in range(10)),
            encoding="utf-8",
        )
        (upstream / "crates" / "boundaries" / "src").mkdir(parents=True)
        (upstream / "crates" / "boundaries" / "src" / "lib.rs").write_text(
            "pub fn check() -> bool {\n    true\n}\n// line4\n",
            encoding="utf-8",
        )
        (upstream / "crates" / "beta" / "src").mkdir(parents=True)
        (upstream / "crates" / "beta" / "src" / "lib.rs").write_text(
            "".join(f"pub const B{i}: usize = {i};\n" for i in range(6)),
            encoding="utf-8",
        )
        (upstream / "crates" / "gamma" / "src").mkdir(parents=True)
        (upstream / "crates" / "gamma" / "src" / "lib.rs").write_text(
            "pub const G0: usize = 0;\n",
            encoding="utf-8",
        )
        (upstream / "patients").mkdir(parents=True)
        (upstream / "patients" / "pt-001.json").write_text(
            '{\n  "id": "pt-001",\n  "synthetic": true\n}\n',
            encoding="utf-8",
        )
        (upstream / "crates" / "mobile-wire" / "src").mkdir(parents=True)
        (upstream / "crates" / "mobile-wire" / "src" / "generated.rs").write_text(
            "// generated wire output should be excluded\n" * 50,
            encoding="utf-8",
        )
        commit_as(
            upstream,
            "Tyler Port",
            "4147075+tyler274@users.noreply.github.com",
            "2026-11-21T08:00:00Z",
            "Initial upstream Tyler commit",
        )

        # Add Dev Patale lines to beta (4 lines -> beta is 6/10 = 60% Tyler -> Tier 3)
        # and 1 coincidental closing line in boundaries (3/4 -> wait: 4/5 = 80% reclaimed).
        with (upstream / "crates" / "beta" / "src" / "lib.rs").open("a", encoding="utf-8") as fh:
            for i in range(6, 10):
                fh.write(f"pub const B{i}: usize = {i};\n")
        with (upstream / "crates" / "boundaries" / "src" / "lib.rs").open(
            "a", encoding="utf-8"
        ) as fh:
            fh.write("// coincidental brace\n")
        commit_as(
            upstream,
            "Dev Patale",
            "republicd25@gmail.com",
            "2026-11-22T09:30:00Z",
            "Dev additions",
        )

        # Add Elijah Gwynn lines to gamma (3 lines -> gamma is 1/4 = 25% Tyler -> Tier 4).
        with (upstream / "crates" / "gamma" / "src" / "lib.rs").open("a", encoding="utf-8") as fh:
            for i in range(1, 4):
                fh.write(f"pub const G{i}: usize = {i};\n")
        commit_as(
            upstream,
            "Elijah Gwynn",
            "eli@egwynn.com",
            "2026-11-23T14:15:00Z",
            "Elijah additions",
        )

        # Add automated dependency bot line in Cargo.lock (Other).
        (upstream / "Cargo.lock").write_text("version = 4\n", encoding="utf-8")
        commit_as(
            upstream,
            "dependabot[bot]",
            "49699333+dependabot[bot]@users.noreply.github.com",
            "2026-11-24T16:45:00Z",
            "Bump lockfile",
        )

        return website, upstream

    def test_multi_author_blame_tiers_and_determinism(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            website, upstream = self._create_synthetic_repos(root)
            out_path = website / "crates" / "ui" / "data" / "ownership-metrics.json"

            rc1 = update_ownership.main(
                [
                    "--repo",
                    str(website),
                    "--output",
                    str(out_path),
                    "--upstream-repo",
                    str(upstream),
                    "--upstream-rev",
                    "HEAD",
                    "--workers",
                    "4",
                ]
            )
            self.assertEqual(rc1, 0)
            first_bytes = out_path.read_bytes()

            # Second run on identical revisions must be 100% byte-identical.
            rc2 = update_ownership.main(
                [
                    "--repo",
                    str(website),
                    "--output",
                    str(out_path),
                    "--upstream-repo",
                    str(upstream),
                    "--upstream-rev",
                    "HEAD",
                    "--workers",
                    "4",
                ]
            )
            self.assertEqual(rc2, 0)
            self.assertEqual(first_bytes, out_path.read_bytes())

            data = json.loads(first_bytes.decode("utf-8"))
            self.assertEqual(data["schema_version"], 1)
            # Latest commit timestamp across both repos is upstream's 2026-11-24T16:45:00Z.
            self.assertEqual(data["generated_at"], "2026-11-24T16:45:00Z")

            # Website assertions (binaries skipped, empty file allowed, 100% Tyler).
            ws = data["website"]
            self.assertEqual(ws["repo"], "DabneyMoeWebsite")
            self.assertEqual(ws["total_files"], 2)  # lib.rs + empty.txt (logo.png skipped)
            self.assertEqual(ws["total_lines"], 3)
            self.assertEqual(ws["tyler_lines"], 3)
            self.assertEqual(ws["tyler_pct"], 100.0)
            self.assertEqual(ws["external_lines"], 0)

            # Upstream assertions (mobile-wire excluded!).
            up = data["upstream"]
            self.assertEqual(up["name"], "dabney-moe")
            # Source files: alpha(10), boundaries(5), beta(10), gamma(4), Cargo.lock(1) = 30 lines, 5 files.
            self.assertEqual(up["source_files"], 5)
            self.assertEqual(up["source_lines"], 30)
            # Tyler source lines: alpha(10) + boundaries(4) + beta(6) + gamma(1) = 21 lines.
            self.assertEqual(up["source_tyler_lines"], 21)
            self.assertEqual(up["source_tyler_pct"], 70.0)
            # Provenance-adjusted adds 1 coincidental line in reclaimed `crates/boundaries` -> 22/30 = 73.3%.
            self.assertEqual(up["source_provenance_adjusted_pct"], 73.3)
            self.assertGreaterEqual(up["source_provenance_adjusted_pct"], up["source_tyler_pct"])

            # Patients: 1 file, 4 lines, 4 Tyler lines.
            self.assertEqual(up["patients_files"], 1)
            self.assertEqual(up["patients_lines"], 4)
            self.assertEqual(up["patients_tyler_lines"], 4)

            # Combined: 6 files, 34 lines, 25 Tyler lines (73.5%).
            self.assertEqual(up["combined_files"], 6)
            self.assertEqual(up["combined_lines"], 34)
            self.assertEqual(up["combined_tyler_lines"], 25)
            self.assertEqual(up["combined_tyler_pct"], 73.5)

            # Authors classified accurately.
            author_map = {a["author"]: a["combined_lines"] for a in up["authors"]}
            self.assertEqual(
                author_map,
                {
                    "Tyler Port": 25,
                    "Dev Patale": 5,
                    "Elijah Gwynn": 3,
                    "Other": 1,
                },
            )

            # Dynamic tier distribution:
            # reclaimed: boundaries + patients/ = 2
            # tier1_2: alpha = 1
            # tier3: beta = 1
            # tier4: gamma + <root>(Cargo.lock 0% Tyler) = 2
            tier_counts = {t["key"]: t["count"] for t in up["tiers"]}
            self.assertEqual(
                tier_counts,
                {"reclaimed": 2, "tier1_2": 1, "tier3": 1, "tier4": 2},
            )

            # --check passes with exit 0.
            rc_check = update_ownership.main(
                [
                    "--repo",
                    str(website),
                    "--output",
                    str(out_path),
                    "--upstream-repo",
                    str(upstream),
                    "--check",
                ]
            )
            self.assertEqual(rc_check, 0)

            # Committing ONLY ownership-metrics.json must NOT cause drift on the next --check!
            commit_as(
                website,
                "dabney-ci",
                "ci@dabney.moe",
                "2026-11-25T06:00:00Z",
                "chore(ownership): refresh automated code ownership metrics",
            )
            rc_after_autocommit = update_ownership.main(
                [
                    "--repo",
                    str(website),
                    "--output",
                    str(out_path),
                    "--upstream-repo",
                    str(upstream),
                    "--check",
                ]
            )
            self.assertEqual(rc_after_autocommit, 0)

    def test_check_fails_on_drift_without_modifying_file(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            website, upstream = self._create_synthetic_repos(root)
            out_path = website / "crates" / "ui" / "data" / "ownership-metrics.json"

            # Missing file -> exit 1.
            stderr_buf = io.StringIO()
            with mock.patch("sys.stderr", stderr_buf):
                rc_missing = update_ownership.main(
                    [
                        "--repo",
                        str(website),
                        "--output",
                        str(out_path),
                        "--upstream-repo",
                        str(upstream),
                        "--check",
                    ]
                )
            self.assertEqual(rc_missing, 1)
            self.assertFalse(out_path.exists())

            # Generate fresh file, then mutate it.
            self.assertEqual(
                update_ownership.main(
                    [
                        "--repo",
                        str(website),
                        "--output",
                        str(out_path),
                        "--upstream-repo",
                        str(upstream),
                    ]
                ),
                0,
            )
            tampered = out_path.read_text(encoding="utf-8").replace("73.5", "12.3")
            out_path.write_text(tampered, encoding="utf-8")

            stderr_buf = io.StringIO()
            with mock.patch("sys.stderr", stderr_buf):
                rc_drift = update_ownership.main(
                    [
                        "--repo",
                        str(website),
                        "--output",
                        str(out_path),
                        "--upstream-repo",
                        str(upstream),
                        "--check",
                    ]
                )
            self.assertEqual(rc_drift, 1)
            self.assertIn("ownership metrics out of date", stderr_buf.getvalue())
            # --check must not overwrite the file.
            self.assertEqual(out_path.read_text(encoding="utf-8"), tampered)

    def test_allow_offline_fallback_and_env_url_clone(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            website, upstream = self._create_synthetic_repos(root)
            out_path = website / "crates" / "ui" / "data" / "ownership-metrics.json"

            # Clone upstream via OWNERSHIP_UPSTREAM_REPO_URL env var (file:// URL).
            with mock.patch.dict(
                os.environ, {"OWNERSHIP_UPSTREAM_REPO_URL": upstream.as_uri()}, clear=False
            ):
                rc = update_ownership.main(
                    [
                        "--repo",
                        str(website),
                        "--output",
                        str(out_path),
                    ]
                )
            self.assertEqual(rc, 0)
            initial = json.loads(out_path.read_text(encoding="utf-8"))
            cached_upstream = initial["upstream"]

            # Add a new Tyler commit to website.
            (website / "crates" / "ui" / "src" / "extra.rs").write_text(
                "pub const EXTRA: u32 = 42;\n",
                encoding="utf-8",
            )
            new_ws_sha = commit_as(
                website,
                "tyler",
                "4147075+tyler274@users.noreply.github.com",
                "2026-11-25T10:00:00Z",
                "Add extra website file",
            )

            # With unreachable upstream URL + --allow-offline, keep cached upstream & refresh website.
            stderr_buf = io.StringIO()
            with mock.patch("sys.stderr", stderr_buf), mock.patch.dict(
                os.environ,
                {"OWNERSHIP_UPSTREAM_REPO_URL": "file:///nonexistent/path/does-not-exist.git"},
                clear=False,
            ):
                rc_offline = update_ownership.main(
                    [
                        "--repo",
                        str(website),
                        "--output",
                        str(out_path),
                        "--allow-offline",
                    ]
                )
            self.assertEqual(rc_offline, 0)
            updated = json.loads(out_path.read_text(encoding="utf-8"))
            self.assertEqual(updated["website"]["revision"], new_ws_sha)
            self.assertEqual(updated["website"]["total_lines"], 4)
            self.assertEqual(updated["website"]["tyler_pct"], 100.0)
            self.assertEqual(updated["upstream"], cached_upstream)
            # Website timestamp (2026-11-25T10:00:00Z) is newer than upstream (2026-11-24T16:45:00Z).
            self.assertEqual(updated["generated_at"], "2026-11-25T10:00:00Z")

            # Without --allow-offline and no upstream URL, command must fail with exit 1.
            stderr_buf = io.StringIO()
            with mock.patch("sys.stderr", stderr_buf), mock.patch.dict(
                os.environ, {"OWNERSHIP_UPSTREAM_REPO_URL": ""}, clear=False
            ):
                rc_fail = update_ownership.main(
                    [
                        "--repo",
                        str(website),
                        "--output",
                        str(out_path),
                    ]
                )
            self.assertEqual(rc_fail, 1)

    def test_committed_ownership_metrics_json_schema_and_hygiene(self) -> None:
        metrics_path = REPO_ROOT / "crates" / "ui" / "data" / "ownership-metrics.json"
        self.assertTrue(
            metrics_path.is_file(),
            f"expected generated metrics file at {metrics_path}",
        )
        raw = metrics_path.read_text(encoding="utf-8")
        lowered = raw.lower()
        for forbidden in update_ownership.FORBIDDEN_OUTPUT_TOKENS:
            self.assertNotIn(forbidden, lowered)

        data = json.loads(raw)
        self.assertEqual(data["schema_version"], 1)
        self.assertRegex(data["generated_at"], r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$")

        ws = data["website"]
        self.assertEqual(ws["repo"], "DabneyMoeWebsite")
        self.assertRegex(ws["revision"], r"^[0-9a-f]{40}$")
        self.assertTrue(ws["revision"].startswith(ws["short_revision"]))
        self.assertEqual(ws["tyler_pct"], 100.0)
        self.assertEqual(ws["external_lines"], 0)
        self.assertGreater(ws["total_files"], 0)
        self.assertGreater(ws["total_lines"], 0)
        self.assertEqual(ws["tyler_lines"], ws["total_lines"])

        up = data["upstream"]
        self.assertEqual(up["name"], "dabney-moe")
        self.assertRegex(up["revision"], r"^[0-9a-f]{40}$")
        self.assertTrue(up["revision"].startswith(up["short_revision"]))
        self.assertEqual(up["combined_files"], up["source_files"] + up["patients_files"])
        self.assertEqual(up["combined_lines"], up["source_lines"] + up["patients_lines"])
        self.assertEqual(
            up["combined_tyler_lines"],
            up["source_tyler_lines"] + up["patients_tyler_lines"],
        )
        for pct_key in (
            "source_tyler_pct",
            "source_provenance_adjusted_pct",
            "combined_tyler_pct",
        ):
            self.assertGreaterEqual(up[pct_key], 0.0)
            self.assertLessEqual(up[pct_key], 100.0)
        self.assertGreaterEqual(up["source_provenance_adjusted_pct"], up["source_tyler_pct"])
        self.assertTrue(up["authors"])
        self.assertEqual(len(up["tiers"]), 4)

        self.assertTrue(data["before_after"])
        for row in data["before_after"]:
            self.assertGreaterEqual(row["before_pct"], 0.0)
            self.assertLessEqual(row["before_pct"], 100.0)
            self.assertGreaterEqual(row["after_pct"], 0.0)
            self.assertLessEqual(row["after_pct"], 100.0)

        self.assertTrue(data["replacement_ledger"])
        self.assertTrue(data["rea_evidence"])
        self.assertEqual(
            data["total_bundles"],
            sum(rec["bundles"] for rec in data["rea_evidence"]),
        )
        for rec in data["rea_evidence"]:
            self.assertRegex(rec["evidence_id"], r"^ev_[0-9a-f]{64}$")
            self.assertGreater(rec["bundles"], 0)


if __name__ == "__main__":
    unittest.main()
