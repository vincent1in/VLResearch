---
name: orx-writing
description: "Edit prose for precision and strip machine-writing patterns: inflated claims, vague attribution, stock phrasing, staged reveals, slogans, recap boxes. Use equations only when needed, prefer figures, derive step by step with one meaning per symbol. Use before finishing a paper, report, finding, brief, README or long reply, when explaining math, or when revising a manuscript or removing AI slop."
---

## Scope

Use this skill on anything a reader will keep: papers and preprints, reports,
finding claims and assessments, briefs, READMEs and docs, and replies longer
than a few paragraphs. For a short reply, apply sections 3 and 4 in your head
and skip the scanner.

When revising the user's text, improve it within the scope they asked for.
Keep its purpose, audience, voice, structure, document class, bibliography,
notation and substantive claims unless the task calls for a change. A paper
need not become a survey or a tutorial. Scale the edit to the request, and do
not invent page limits, standard sections, boxes or extra deliverables.

## 1. Read before editing

- Read the actual root file, the included files you will touch, and the text
  around each edit. Identify the build engine, the intended reader and the
  scope. For a focused edit, read enough to keep notation and terms
  consistent; leave unrelated material alone.
- Note the author's conventions: spelling variety, tense, person, notation,
  macro names, how results and citations are phrased. Edits follow them.

## 2. Get the content right first

Style fixes cannot rescue a wrong or unsupported claim, so check substance
before polishing sentences.

- Define nontrivial objects and state assumptions before they are used.
- For technical claims in scope, check derivations, conditions, units,
  signs and citations. Keep qualifications and distinctions the author made.
- Every number you report comes from a run, a log line or a cited source
  (`orx-results`, `orx-logs`). Say how many trials stand behind a best
  result. Write "significant" only with the test, and give intervals or
  standard errors where they exist.
- Repair a demonstrable gap when asked and feasible. Otherwise name the
  problem and a concrete correction, with the certainty you actually have.
- When sources are supplied for integration, read the relevant full texts
  and appendices, check bibliographic details, and track page, theorem and
  notation correspondences. Keep attribution, disagreements and limits;
  separate reported evidence, illustrative calculations and new results.
  Say which sources you could not open rather than implying you read them.
  Never invent a citation, a quote or an empirical result.

## 3. Equations, figures and notation

Explain with a figure or a sentence before reaching for an equation. Write
an equation only when the reader needs the exact relation: a definition that
later steps use, a derivation the claim rests on, or a formula someone will
implement or check.

- If a sentence states the relation exactly, write the sentence: "the score
  is the mean of the five fold scores" needs no summation.
- If the point is a shape, a trend, a trade-off or a mechanism, show it with
  a plot, a diagram or a small worked example in real numbers. Make figures
  with `orx-figures`; its diagram reference covers schematics.
- If the exact form matters as well, give both. The figure carries the
  intuition and the equation the exact statement, and the figure's axes,
  curves and parts are labeled with the equation's symbols so the reader can
  match one to the other.

When a derivation is needed, walk through it so the reader can check each
step without filling a gap:

- One move per step, with a few words saying what changed and why it is
  allowed: "substitute (2) into (1)"; "take expectations; the cross term is
  zero because $\varepsilon$ has mean zero and is independent of $x$".
- Name the identity, assumption or cited theorem at the step that uses it.
  Number only the equations you refer to again.
- After the last step, say what the result means for the quantity the reader
  cares about. A small numeric example after a general result often makes
  it concrete.

Keep the notation stable from one step to the next:

- Define each symbol in words where it first appears, with its shape or
  units when they are not obvious: "$X$, the $n \times p$ matrix of
  features".
- One symbol, one meaning, across the whole document. If $\lambda$ is the
  penalty, an eigenvalue gets another letter, even in a section where the
  penalty does not appear. One object also keeps one symbol in the text,
  figures, tables and captions; when the code names it differently (`alpha`
  for $\lambda$), say so once.
- Do not tell unrelated objects apart by case or font alone: $k$ for a fold
  and $K$ for the number of features, $T$ for the number of periods and
  $X^T$ for a transpose (write $X^\top$), $x$ and $\mathbf{x}$ for different
  things. Case and font may carry meaning only under one stated convention
  used everywhere, such as $X$ for a matrix and $x_i$ for its $i$th row, or
  $Y$ for a random variable and $y$ for its value.
- When a step transforms an object (centering, rescaling, taking a subset,
  conditioning), give the result a new mark and say how it relates to the
  original: "let $\tilde{X}$ be $X$ with each column centered". Never keep
  the old symbol for the new object. Give each mark one job: a hat for an
  estimate, a bar for an average, a tilde for a transformed copy.
- Keep index letters fixed ($i$ for observations, $t$ for periods), and do
  not sum over a letter that already names something.
- When a symbol returns after a gap of a page or a section, restate it in a
  few words: "the penalty $\lambda$". With many symbols, add a notation
  table near the start.
- When revising the user's text, keep their notation. Point out a collision
  and propose a fix; rename throughout only within the scope they asked for.

Section 4 of [references/patterns.md](references/patterns.md) has a worked
example of each point.

## 4. Edit for precision and economy

Follow the plain-style principles of Zinsser's *On Writing Well*: cut words
that do no work, prefer the concrete term, and let the content carry the
emphasis.

Read the draft once against the checklist below. Each item is a prompt for
judgment, not a ban and not evidence of who wrote the text. Change an
instance when it is repetitive, vague, overstated or distracting. Keep a
contrast, parallel, technical term or dash that serves the argument. Do not
flatten a distinctive voice, and never swap a precise technical word for a
simpler wrong one.

| Pattern | Typical form | Usual fix |
| --- | --- | --- |
| Inflated significance | "a pivotal result", "groundbreaking" | State the contribution and its evidence |
| Vague attribution | "studies have shown", "experts agree" | Cite the specific source and claim, or cut |
| Stock filler | "in today's rapidly evolving landscape" | Delete; start with the point |
| Trailing participles | "..., highlighting the importance of X" | Say the causal or mathematical link, or cut |
| Corrective antithesis | "It is about A, not B." on repeat | State A; mention B once where it helps |
| Staged misconception | "X is often described as Y. That is incomplete." | Give the correct account directly |
| Unprompted rebuttal | "That is not an omission." | Give the reason without the defense |
| Counted packages | "Three lessons stand out" | Drop the count; present the items |
| Slogan headings | "Structure beats tuning." | A descriptive heading |
| Explaining the explanation | "What the bound says", "Why it holds" | Fewer, plainer headings |
| Self-staged questions | "This raises a question: ..." | Make the statement |
| Announced importance | "This distinction matters." | Show the consequence |
| Tour-guide transitions | "Back to finance." | A substantive link or none |
| Repeated metaphor | "what X buys", "prices the signal" | Literal description |
| Agency for math objects | ridge "decides", a direction "prefers" | Describe the behavior |
| Mirror sentences | "A does p, and B does q." | Keep only if the claim really is that clean |
| Consultancy and moral words | "playbook", "audit", "honest validation" | Name the procedure or the property |
| Theatrical disclaimers | "not every conceivable case" | State the limit plainly |
| Repeated caveats and recap boxes | the same point twice within a page | Keep the better instance |
| Admonitions after a result | "More compute is not always better." | Let the result stand |
| Mechanical uniformity | same paragraph template, rhythm, transition | Vary by content, not decoration |

[references/patterns.md](references/patterns.md) has before-and-after
examples for each row, the reply-specific patterns, and the sources.

## 5. Check the draft

Run the scanner on the files you edited; it lists candidate lines, one per
pattern hit, and changes nothing:

```sh
orx skill writing/assets/prose_scan.py > /tmp/prose_scan.py
python3 /tmp/prose_scan.py paper.tex docs/report.md
```

A hit is a place to look, not a verdict: keep a line when the pattern serves
the text. Patterns of structure (repeated caveats, recap boxes, staged
questions) need a full read; the scanner sees only phrases, not notation.

If the text has equations, list its symbols with each one's meaning and
where it is defined. Fix any symbol with two meanings, any object with two
symbols, any pair told apart only by case or font outside the stated
convention, and any symbol used before it is defined. Check that each figure
uses the symbols of the equations it illustrates, and check a derivation's
result on small numbers (a few lines of Python) when the algebra is not
obvious. Last, ask of each remaining display whether a sentence or a figure
would carry it.

Then re-read the edited passages in order. Check that every claim you kept
still has its evidence and qualifiers, that terms and notation match the rest
of the document, and that no paragraph now repeats another.

## 6. Build and report

- For LaTeX, compile the actual root with the project's engine as
  `orx-paper` describes, read the log for new warnings, and look at the
  pages you changed. Keep the project's macros and build conventions; define
  new macros locally rather than editing a shared macro file.
- Report what you changed, what you checked, and any material problem you
  could not resolve. Do not summarize the edits with the patterns above.
