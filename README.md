# VLResearch

A local-first workspace for research agents, tuned for quantitative research: adding datasets, updating models, and making processes faster, with experiments run on your own machines over SSH.

VLResearch is a distribution of [OpenResearch](https://github.com/alphaXiv/OpenResearch) by alphaXiv. It keeps OpenResearch's experiment tree, SSH and Slurm compute, and chat with your coding agent (Claude Code, Codex or OpenCode), and changes three things:

- **Nothing leaves your machine except SSH and your LLM.** orx itself never calls an LLM; your coding agent does. VLResearch blocks what orx would otherwise send on its own: update checks, accounts and managed compute, literature APIs, GitHub, Overleaf and agent installers. It also turns off the agents' own telemetry and auto-updates.
- **Chats hear back from their runs.** Every run a chat launches wakes that chat when it finishes, instead of only when the agent remembered to ask.
- **Waiting is visible.** When Claude Code is retrying a failed API call, the chat shows a retry row instead of a silent hang.

Planned next: a metrics ledger, an idea queue, a "grill me" scoping step before any compute is spent, a forecast-and-verify improvement loop, and a findings ledger.

## Install from source

You need a stable Rust toolchain and git. VLResearch has no release builds, and it never installs alphaXiv's releases over itself.

```sh
git clone https://github.com/vincent1in/VLResearch.git
cd VLResearch
cargo install --path . --locked
orx up
```

`orx up` opens the dashboard at `http://127.0.0.1:4791`. The command is still called `orx`, as in OpenResearch.

Install and sign in to your coding agent yourself before the first run; VLResearch won't download it for you. To update, pull, run `cargo install --path . --locked` again, and restart `orx up`.

## Network policy

| Leaves the machine | How |
| --- | --- |
| SSH to your compute hosts | Your `~/.ssh/config`, as in OpenResearch |
| Calls to your LLM provider | Made by your coding agent, signed in with its own credentials |
| Anything else orx sends on its own | Blocked, unless you list the host in `VLR_EGRESS_ALLOW` |

Two things still connect when you choose to use them: the command-line tools of third-party compute backends you configure (Modal, Hugging Face, Kubernetes), and tectonic, which downloads a TeX bundle if it is the only LaTeX engine installed.

orx also can't police what the agent runs in its own shell. To make the policy hold for the whole machine, allow only your LLM provider (port 443) and your SSH hosts (port 22) at the firewall.

## Settings

All optional. Set them in the environment `orx up` starts from.

| Variable | Default | Effect |
| --- | --- | --- |
| `VLR_EGRESS_ALLOW` | empty | Hosts orx may reach besides this machine, comma-separated: `export.arxiv.org`, `*.example.com`, or `*` for no limit |
| `VLR_GIT_PROTOCOLS` | `file` | Git transports orx may use; `file:ssh` adds an internal git server over SSH |
| `VLR_ALLOW_GITHUB` | off | Turns GitHub integration (repo creation, branch pushes) back on |
| `VLR_ALLOW_INSTALLERS` | off | Lets the dashboard install or update coding agents |
| `VLR_AUTO_WAKE` | on | `0` keeps OpenResearch's opt-in wake-ups |
| `VLR_AGENT_ENV_DEFAULTS` | on | `0` stops orx adding the no-telemetry settings to agent processes |

## Keys and secrets

No key, token, password or hostname belongs in this repository, and nothing here needs one. Agents keep their logins in their own config folders, and SSH uses your `~/.ssh/config`.

- Run `flavor/scripts/install-hooks.sh` once per clone. It refuses commits that contain credentials; it needs [gitleaks](https://github.com/gitleaks/gitleaks).
- CI scans every file VLResearch adds or changes.
- On GitHub, turn on secret scanning and push protection in the repository's security settings.

## Staying current with OpenResearch

OpenResearch releases every few days. `flavor/scripts/sync-upstream.sh` merges the newest release on a review branch, rebuilds generated files, and runs the checks. VLResearch's changes to upstream files are short hooks marked `VLResearch:`, so conflicts stay small. [VLRESEARCH.md](VLRESEARCH.md) covers the layout and the procedure.

## License

MIT, like OpenResearch. See [LICENSE](LICENSE) and [NOTICE](NOTICE).
