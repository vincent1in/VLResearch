#!/usr/bin/env python3
"""List lines that match common machine-writing patterns.

Usage: python3 prose_scan.py FILE [FILE ...]

Prints one line per hit: path:line: [pattern] text. A hit is a place to look,
not a verdict; the file is never changed. Exit status is 0 whether or not
anything matched (2 if a file can't be read).
"""

import re
import sys

# (label, regex). Case-insensitive; matched against one line at a time.
PATTERNS = [
    ("inflated significance", r"\b(pivotal|groundbreaking|transformative|game[- ]chang\w*|revolutioni[sz]\w*|a testament to|paradigm shift)\b"),
    ("vague attribution", r"\b(studies (have )?show\w*|experts (agree|say|believe)|it is widely (believed|known|accepted)|research (has )?show\w*)\b"),
    ("stock filler", r"\b(rapidly evolving|ever[- ]evolving|in today's|plays? a (crucial|vital|key|pivotal) role|rich tapestry|landscape of|navigat\w+ the complexit\w*)\b"),
    ("trailing participle", r",\s*(highlighting|underscoring|emphasi[sz]ing|showcasing|reflecting|demonstrating|illustrating) (the|its|their|how)\b"),
    ("buys/prices metaphor", r"\b(what \w+ buys|\w+ can buy|how much \w+ buys|purchased by|prices (the|hidden))\b"),
    ("counted package", r"\b(two|three|four|five|six) (lessons|takeaways|facts|things|insights|ideas) (stand out|emerge|matter)\b|\badds (two|three|four) (lessons|things)\b"),
    ("compression claim", r"\b(in one sentence|fits in one sentence|in a nutshell|boils down to)\b"),
    ("staged reveal", r"\b(that (description|picture|view|story) is incomplete|the real (answer|story) is|but here's the thing)\b"),
    ("unprompted rebuttal", r"\b(that is not an omission|is not a contradiction|this is not a bug)\b"),
    ("announced importance", r"\b(this (distinction|point|detail|difference) matters|it is (crucial|important|essential) to (note|remember|understand))\b"),
    ("self-staged question", r"\b(raises (an|a further|an additional|the) question|here the question changes|the natural question is)\b"),
    ("tutorial heading", r"^\s*(#+\s*|\\(sub)*section\*?\{)\s*(what|why|how) (the|it|this) [\w\s-]{0,30}(says|holds|works|means)\b"),
    ("tour-guide transition", r"^\s*back to [\w\s]{1,20}\.\s*$"),
    ("chapter promise", r"\bthe next (chapter|section) (will )?(measures?|identif\w+|shows?|explores?)\b"),
    ("moralized precision", r"\bhonest (constants?|validation|evaluation|estimates?|baselines?)\b"),
    ("consultancy word", r"\b(playbook|operationali[sz]e|actionable insights?|best-in-class|synerg\w+)\b"),
    ("theatrical disclaimer", r"\bevery conceivable\b"),
    ("hedge stack", r"\b(may|might|could) (potentially|possibly)\b|\bpotentially (suggest|indicate)\w*\b"),
    ("chat opener/closer", r"^\s*(great question|certainly!|absolutely!|i hope this helps|let me know if you (have|need|want))\b"),
    ("agency verb", r"\b(ridge|lasso|the (estimator|optimizer|penalty|model|eigen\w+)) (decides|prefers|wants|chooses|knows)\b"),
]

COMPILED = [(label, re.compile(rx, re.IGNORECASE)) for label, rx in PATTERNS]


def scan(path):
    with open(path, encoding="utf-8", errors="replace") as handle:
        for number, line in enumerate(handle, 1):
            text = line.rstrip("\n")
            if text.lstrip().startswith("%"):  # LaTeX comment
                continue
            for label, rx in COMPILED:
                if rx.search(text):
                    shown = text.strip()
                    if len(shown) > 160:
                        shown = shown[:157] + "..."
                    print(f"{path}:{number}: [{label}] {shown}")


def main(paths):
    if not paths:
        print(__doc__.strip(), file=sys.stderr)
        return 2
    status = 0
    for path in paths:
        try:
            scan(path)
        except OSError as error:
            print(f"{path}: {error}", file=sys.stderr)
            status = 2
    return status


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
