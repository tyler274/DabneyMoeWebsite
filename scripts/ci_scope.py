#!/usr/bin/env python3
"""Select which CI jobs a change needs, from the complete Git comparison.

Prints `build=<bool>` and `android=<bool>` to $GITHUB_OUTPUT (or stdout).

- build:   false only when every changed path is documentation or out-of-band
           infrastructure that no build, test, image, or deploy step reads.
- android: true when the change can affect the Android APK.

Anything that cannot be compared (manual dispatch, first push of a branch,
missing history, act) selects everything.
"""

import json
import os
from pathlib import Path
import re
import subprocess

# Terraform and the self-host stack are applied by hand, never by this workflow.
SKIP_PREFIXES = ("terraform/", "selfhost/")

# Inputs of the Android build: the Tauri shell, the crates it links, the Trunk
# frontend bundle (styles + copied public assets), and the Nix toolchain.
ANDROID_PREFIXES = ("src-tauri/", "crates/", "nix/", "public/", "style/")
ANDROID_FILES = {
    "Cargo.lock",
    "Cargo.toml",
    "Trunk.toml",
    "flake.lock",
    "flake.nix",
    "rust-toolchain.toml",
    "tailwind.config.js",
    ".github/workflows/ci.yml",
    "scripts/ci_scope.py",
}


def skippable(path):
    return path.endswith(".md") or path.startswith(SKIP_PREFIXES)


def android_input(path):
    return path in ANDROID_FILES or path.startswith(ANDROID_PREFIXES)


def git(*args):
    return subprocess.check_output(["git", *args], stderr=subprocess.PIPE)


def changed_paths(event_name, event):
    """Return (paths, None) or (None, reason the comparison is unavailable)."""
    try:
        if event_name == "pull_request":
            base = event["pull_request"]["base"]["sha"]
            head = event["pull_request"]["head"]["sha"]
        elif event_name == "push":
            base, head = event["before"], event["after"]
        else:
            return None, f"{event_name or 'unknown'} event runs everything"
        if not all(
            isinstance(sha, str) and re.fullmatch(r"[0-9a-f]{40}", sha) and sha != "0" * 40
            for sha in (base, head)
        ):
            return None, "comparison revisions unavailable"
        if event_name == "pull_request":
            base = git("merge-base", base, head).decode().strip()
        # No rename detection: both the deleted and the added path must qualify.
        changed = git("diff", "--name-only", "--no-renames", "-z", base, head, "--")
    except (KeyError, TypeError, subprocess.CalledProcessError):
        return None, "complete comparison unavailable"
    paths = [os.fsdecode(path) for path in changed.split(b"\0") if path]
    if not paths:
        return None, "empty comparison"
    return paths, None


def select(event_name, event):
    """Return (build, android, reason)."""
    paths, unavailable = changed_paths(event_name, event)
    if paths is None:
        return True, True, unavailable
    if all(skippable(path) for path in paths):
        return False, False, "only documentation, terraform/ or selfhost/ changed"
    if any(android_input(path) for path in paths):
        return True, True, "change includes Android build inputs"
    return True, False, "change cannot affect the Android build"


def main():
    try:
        event = json.loads(Path(os.environ["GITHUB_EVENT_PATH"]).read_text())
    except (KeyError, OSError, ValueError):
        event = {}
    build, android, reason = select(os.environ.get("GITHUB_EVENT_NAME", ""), event)
    output = f"build={str(build).lower()}\nandroid={str(android).lower()}\n"
    summary = f"CI scope: build={build}, android={android} ({reason})"
    print(summary)
    if os.environ.get("GITHUB_OUTPUT"):
        with open(os.environ["GITHUB_OUTPUT"], "a") as stream:
            stream.write(output)
    else:
        print(output, end="")
    if os.environ.get("GITHUB_STEP_SUMMARY"):
        with open(os.environ["GITHUB_STEP_SUMMARY"], "a") as stream:
            stream.write(summary + "\n")


if __name__ == "__main__":
    main()
