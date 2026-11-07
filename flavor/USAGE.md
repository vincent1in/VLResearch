# Usage profiles

Each researcher who runs VLResearch keeps one usage profile: a Markdown file on their own machine for what is theirs alone. It holds their instructions, the corrections they have given, the settings they chose on purpose, facts about their setup, and adaptations learned from their own work, each with a rule for what happens to it when OpenResearch or VLResearch updates. Changes that make research cheaper on any install go in the efficiency ledger, [EFFICIENCY.md](EFFICIENCY.md), and reach other installs only by consensus. Nothing in a profile reaches another install.

This file describes the format and how orx will use it (plan addition A21, not built yet). A blank profile to copy is at the end.

## Where a profile lives

```text
~/.config/openresearch/vlresearch/usage.md
```

That is orx's config directory (`$XDG_CONFIG_HOME/openresearch`, or `~/.config/openresearch` on every platform; `src/config.rs` L78), chosen because nothing that updates VLResearch touches it:

- The config directory never moves. A data-directory move leaves it alone, and an update, an upstream merge or a reinstall changes only code.
- It is outside every repository, so no merge or pull request changes it and nothing in it is published.
- Bundled skills (`orx-*`, VLResearch's six included) are written into the worktree from the binary on every turn, and an uploaded skill can never take an `orx-` name (`src/local/user_skills.rs` L284–302). A researcher's change to how a VLResearch skill behaves is therefore written here, or as an uploaded skill under a name of its own, never as an edit to a bundled skill, which the next turn would overwrite.
- Values saved on the Settings page stay in the store's `vlr_settings` table, which survives updates. The profile records which of them the researcher set on purpose, so that a new default is never applied over them without a word.

On a second machine, copy the file; each machine keeps its own update log.

## How orx will use it (A21)

- **Every turn.** A short `<vlr-usage>` block carries the standing instructions, and only those, within about 300 tokens. It is added by one hook in `with_turn_context` (`src/local/chat/mod.rs`; L1313 at 9f37717, L1389 at v0.2.25, whose signature changed), beside the goal and autonomy blocks, and it rides every turn for the goal's reason: it outlives compaction and a lost native session. The rest of the profile is read when relevant (`orx usage show <id>`), because context files loaded in full raised coding agents' inference cost by over 20% without raising success ([Evaluating AGENTS.md](https://arxiv.org/abs/2602.11988)).
- **Writing.** The researcher edits the file by hand, or accepts a proposal. When the researcher corrects the agent, orx records a proposed item (`orx usage propose`), which the Research view offers to accept, edit or refuse. orx writes the file only on the researcher's acceptance, and agents propose items through orx rather than editing the file.
- **Precedence.** On the researcher's own install, an active item wins over a VLResearch default and over agent instructions in a repository, such as OpenResearch's contributor rules. Each such conflict is written to the update log.
- **After an update.** The update pass below.

Until A21 is built, the standing instructions can go in the coding agent's own global instructions (for Claude Code, its user-level `CLAUDE.md`), which orx links into every agent home it prepares and never edits.

## The format

A profile is plain Markdown: a short header, then these sections in this order. Empty sections stay, so a reader sees what is missing.

1. **How I want updates handled.** The researcher's rules for updates, in their own words. These apply wherever an item does not say otherwise.
2. **Standing instructions.** The items that ride every turn.
3. **Context.** Facts about the researcher's setup: hosts, data, domain, accounts. Read when relevant, not every turn.
4. **Settings and ledger choices.** Settings set on purpose; opt-ins to provisional and opt-in ledger entries; opt-outs of defaults.
5. **Adaptations.** Changes whose gain showed only on this researcher's own work. Provisional by nature.
6. **Proposed.** Items orx or the agent suggested and the researcher has not accepted. Not applied.
7. **Retired.** Items no longer applied, with when and why. Never deleted.
8. **Update log.** One line per update.

Each item is a level-3 heading with an ID and a short title, then fields:

```markdown
### U2 Merge upstream releases; never rebase or force-push
- rule: OpenResearch releases are merged into VLResearch, never rebased, and main is never force-pushed.
- kind: instruction
- scope: the VLResearch repository
- on update: keep
- source: decision recorded in the project notes
- since: 2026-11-07, VLResearch 9f37717 (OpenResearch v0.2.24)
```

**kind** is one of:

| Kind | Meaning |
| --- | --- |
| instruction | Something the researcher said to do or not to do. |
| correction | An instruction given by correcting the agent. It also records what went wrong. |
| context | A fact about the researcher's setup. |
| setting | A value set on purpose, or a choice about a ledger entry. |
| adaptation | A change learned from this researcher's work whose gain showed only there. |

**on update** is one of:

| Rule | After an update | Default for |
| --- | --- | --- |
| `keep` | Carried over unchanged. | instructions, corrections |
| `re-check: <check>` | orx runs the named check: a command, a test, or a question the agent must answer from the profile. Passing keeps the item; failing asks. | context, settings, adaptations |
| `ask` | Shown to the researcher with the release notes that touch its area. | anything the researcher marks |
| `retire when: <condition>` | Moved to Retired once the condition holds: a release ships the same behavior, a ledger entry becomes a default that makes the item redundant, a service starts working again, or a date passes. | workarounds |

**scope** is `all`, a project, a repository or a domain. **source** says who said it, when and where. **since** names the date and the versions the item was last validated on.

## The update pass (A21)

It runs when orx starts and finds a VLResearch commit or OpenResearch version different from the one in the profile's `validated on` line, which is what installing the release watcher's merge does.

1. **Read.** An item that does not parse is kept as text and reported, never dropped.
2. **Apply each rule:** keep, re-check, ask, or retire when.
3. **Settle conflicts.** If the update brings a default or a repository instruction that contradicts an active item, the item wins on this install and the conflict is logged. If the update removes something an item depends on, the item is asked.
4. **Record.** One line in the update log and a new `validated on` line. Nothing else in the file changes without the researcher.
5. **Report once.** One summary in the Research view: kept, re-checked, retired, and waiting for the researcher.

## Whose change is it?

The self-improvement loop sorts every change before testing it:

1. **Did the researcher ask for it, or correct the agent into it?** Then it is a usage item for that researcher, applied at once. If the loop believes the cause is general, it also opens an efficiency candidate, which is tested like any other.
2. **Does it name or depend on something only this researcher has** (paths, hosts, repositories, accounts, identities, datasets, schedules, domain conventions)? Usage.
3. **Does its gain hold on the shared planted panels**, measured on this install? An efficiency candidate, for the ledger. If the gain shows only on this researcher's own studies, it is a usage adaptation for them.

A change with a general mechanism and a personal setting is split: the mechanism goes to the ledger, the setting to the profile.

## What is measured

- **Repeat corrections:** how often the researcher gives an instruction the profile already holds. Target: none.
- **Retention:** the share of active items honored after an update, by each item's check. Target: every `keep` item.
- **Leaks:** profile text, paths, hosts or names found in the repository, the ledger or exported evidence. Target: none, enforced by a CI check.
- **Size:** tokens in the every-turn block.

Benchmark tier L1u tests all four on a planted profile across a simulated upstream merge.

## Blank profile

```markdown
# Usage profile: <name>

- profile format: 1
- owner: <name>
- validated on: VLResearch <commit>, OpenResearch <version>

## How I want updates handled

(Your rules for updates, in your words. Until you write some: keep instructions
and corrections, re-check context, settings and adaptations, and ask about any
conflict.)

## Standing instructions

## Context

## Settings and ledger choices

## Adaptations

## Proposed

## Retired

## Update log

| Date | From | To | Kept | Re-checked | Retired | Asked |
| --- | --- | --- | --- | --- | --- | --- |
```
