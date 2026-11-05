---
name: orx-writing
description: "Edit prose for precision and economy and strip machine-writing patterns: inflated claims, vague attribution, stock phrasing, staged reveals, counted lists, slogans, recap boxes. Use before finishing a paper, report, finding, brief, README or long reply, when revising the user's manuscript, or when asked to remove AI slop or tighten writing. LaTeX builds stay with `orx-paper`."
---

## Scope

Use this skill on anything a reader will keep: papers and preprints, reports,
finding claims and assessments, briefs, READMEs and docs, and replies longer
than a few paragraphs. For a short reply, apply the checklist below in your
head and move on.

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
- Note the author's conventions: spelling variety, tense, person, macro
  names, how results and citations are phrased. Edits follow them.

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

## 3. Edit for precision and economy

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
| Unprompted rebuttal | "That is not an omission." | Give the reason without the defence |
| Counted packages | "Three lessons stand out" | Drop the count; present the items |
| Slogan headings | "Structure beats tuning." | A descriptive heading |
| Explaining the explanation | "What the bound says", "Why it holds" | Fewer, plainer headings |
| Self-staged questions | "This raises a question: ..." | Make the statement |
| Announced importance | "This distinction matters." | Show the consequence |
| Tour-guide transitions | "Back to finance." | A substantive link or none |
| Repeated metaphor | "what X buys", "prices the signal" | Literal description |
| Agency for math objects | ridge "decides", a direction "prefers" | Describe the behaviour |
| Mirror sentences | "A does p, and B does q." | Keep only if the claim really is that clean |
| Consultancy and moral words | "playbook", "audit", "honest validation" | Name the procedure or the property |
| Theatrical disclaimers | "not every conceivable case" | State the limit plainly |
| Repeated caveats and recap boxes | the same point twice within a page | Keep the better instance |
| Admonitions after a result | "More compute is not always better." | Let the result stand |
| Mechanical uniformity | same paragraph template, rhythm, transition | Vary by content, not decoration |

[references/patterns.md](references/patterns.md) has before-and-after
examples for each row, the reply-specific patterns, and the sources.

## 4. Check the draft

Run the scanner on the files you edited; it lists candidate lines, one per
pattern hit, and changes nothing:

```sh
orx skill writing/assets/prose_scan.py > /tmp/prose_scan.py
python3 /tmp/prose_scan.py paper.tex docs/report.md
```

A hit is a place to look, not a verdict: keep a line when the pattern serves
the text. Patterns of structure (repeated caveats, recap boxes, staged
questions) need a full read; the scanner sees only phrases.

Then re-read the edited passages in order. Check that every claim you kept
still has its evidence and qualifiers, that terms and notation match the rest
of the document, and that no paragraph now repeats another.

## 5. Build and report

- For LaTeX, compile the actual root with the project's engine as
  `orx-paper` describes, read the log for new warnings, and look at the
  pages you changed. Keep the project's macros and build conventions; define
  new macros locally rather than editing a shared macro file.
- Report what you changed, what you checked, and any material problem you
  could not resolve. Do not summarize the edits with the patterns above.
