#!/usr/bin/env python3
"""Flags new ways for orx to reach the network outside the egress gate.

reqwest clients can only be built in src/net.rs (clippy enforces that), and
that is where VLResearch's gate lives. Everything else that can open a
connection (websockets, raw sockets, spawning curl or an installer, a fetch to
an absolute URL from the dashboard) is counted per file here and compared with
flavor/egress-baseline.txt. A count above the baseline fails the check, so an
upstream merge that adds one gets a human look before it ships.

    python3 flavor/scripts/egress_audit.py            # check
    python3 flavor/scripts/egress_audit.py --update   # accept the current counts
"""

from __future__ import annotations

import collections
import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parents[2]
BASELINE = ROOT / "flavor" / "egress-baseline.txt"

RUST_PATTERNS = {
    "websocket client": re.compile(r"\bconnect_async\w*\s*\("),
    "raw socket": re.compile(r"\b(?:TcpStream::connect|UdpSocket::bind)\b"),
    "network command": re.compile(
        r'Command::new\(\s*"(?:curl|wget|gh|npm|npx|pnpm|pip3?|uvx?|brew|winget|'
        r'apt(?:-get)?|kubectl|modal|hf|huggingface-cli|powershell|pwsh)"'
    ),
    "piped installer": re.compile(r"\|\s*(?:sh|bash|iex)\b"),
}
UI_PATTERNS = {
    "absolute fetch": re.compile(r"\bfetch\(\s*[`'\"]https?://"),
    "websocket to a remote host": re.compile(r"new WebSocket\(\s*[`'\"]wss?://(?!127\.0\.0\.1|localhost)"),
}


def tracked(*prefixes: str) -> list[str]:
    out = subprocess.run(
        ["git", "ls-files", "--", *prefixes],
        cwd=ROOT,
        check=True,
        capture_output=True,
        text=True,
    ).stdout
    return [line for line in out.splitlines() if line]


def scan() -> dict[tuple[str, str], int]:
    counts: dict[tuple[str, str], int] = collections.Counter()
    for path in tracked("src"):
        if not path.endswith(".rs") or path.startswith("src/flavor/"):
            continue
        text = (ROOT / path).read_text(encoding="utf-8", errors="replace")
        for label, pattern in RUST_PATTERNS.items():
            hits = len(pattern.findall(text))
            if hits:
                counts[(label, path)] += hits
    for path in tracked("ui/src"):
        if not path.endswith((".ts", ".tsx")):
            continue
        text = (ROOT / path).read_text(encoding="utf-8", errors="replace")
        for label, pattern in UI_PATTERNS.items():
            hits = len(pattern.findall(text))
            if hits:
                counts[(label, path)] += hits
    return counts


def read_baseline() -> dict[tuple[str, str], int]:
    counts: dict[tuple[str, str], int] = {}
    if not BASELINE.exists():
        return counts
    for line in BASELINE.read_text(encoding="utf-8").splitlines():
        if not line.strip() or line.startswith("#"):
            continue
        label, path, count = line.split("\t")
        counts[(label, path)] = int(count)
    return counts


def write_baseline(counts: dict[tuple[str, str], int]) -> None:
    lines = [
        "# Accepted network call sites outside the egress gate; see flavor/scripts/egress_audit.py.",
        "# label<TAB>path<TAB>count",
    ]
    lines += [f"{label}\t{path}\t{count}" for (label, path), count in sorted(counts.items())]
    BASELINE.write_text("\n".join(lines) + "\n", encoding="utf-8")


def main() -> int:
    current = scan()
    if "--update" in sys.argv[1:]:
        write_baseline(current)
        print(f"egress audit: baseline updated ({len(current)} entries)")
        return 0
    baseline = read_baseline()
    new = {key: count for key, count in current.items() if count > baseline.get(key, 0)}
    if not new:
        print(f"egress audit: no new network call sites ({len(current)} known)")
        return 0
    print("egress audit: new network call sites outside the egress gate:")
    for (label, path), count in sorted(new.items()):
        print(f"  {path}: {label} ({baseline.get((label, path), 0)} -> {count})")
    print(
        "Check where each one connects. If it is fine (or already blocked), accept it with\n"
        "  python3 flavor/scripts/egress_audit.py --update"
    )
    return 1


if __name__ == "__main__":
    sys.exit(main())
