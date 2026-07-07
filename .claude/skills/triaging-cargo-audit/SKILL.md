---
name: triaging-cargo-audit
description: Use when running `cargo audit` / `just audit` on the tcyb workspace, triaging RustSec (RUSTSEC-*) vulnerability / unsound / unmaintained / yanked findings, or deciding how to fix vs defer a dependency advisory before `just ci`. Symptoms — `error: N vulnerabilities found!`, an advisory only some tools flag, "which crate pulls this in", or whether to bump / `cargo update` / ignore.
---

# Triaging cargo audit

## Overview

`just audit` (= `cargo audit`, config [.cargo/audit.toml](../../../.cargo/audit.toml)) scans `Cargo.lock` against the RustSec advisory DB. But `just ci` also runs `cargo deny check advisories` ([deny.toml](../../../deny.toml)) — **the same DB, different verdicts.** Both must be green for CI.

**Core principle: `cargo audit` green ≠ CI green. A finding can pass audit and fail deny. Always check both tools and classify every finding by which gate it actually blocks before proposing a fix.**

## Step 1 — Run both gates

```
just audit                      # cargo audit — vulnerabilities are errors; unmaintained/unsound/yanked are ALLOWED warnings here
cargo deny check advisories     # unsound + vulnerability are ERRORS here; unmaintained scoped to "workspace"
```
Never conclude from `cargo audit` alone. `deny` output is also richer: it prints the fixed version and the suggested `cargo update -p ...` command per finding.

## Step 2 — Classify each finding (the crux)

The same advisory lands differently in each tool because [.cargo/audit.toml](../../../.cargo/audit.toml) deliberately does **not** deny warnings, while [deny.toml](../../../deny.toml) errors on `unsound`/`vulnerability` and warns `unmaintained = "workspace"`:

| Advisory kind | `cargo audit` | `cargo deny` | Blocks `just ci`? |
|---|---|---|---|
| `vulnerability` | **error** | **error** | Yes (both) |
| `unsound` | warning (allowed) | **error** | **Yes — via deny only** |
| `unmaintained` (workspace/direct dep) | warning | **error** | Yes — via deny |
| `unmaintained` (deep transitive) | warning | warning (out of "workspace" scope) | No |
| `yanked` | warning | warning | No |

The trap: an `unsound` finding on a direct dep (e.g. `anyhow`) looks harmless in `cargo audit` output ("N allowed warnings") yet is a hard CI failure in `deny`. Read the `deny` result before declaring anything non-blocking.

## Step 3 — Find who pulls the crate in

```
cargo tree -i <crate> --target all     # ALWAYS pass --target all on Windows
```
**Windows gotcha:** `cargo tree -i <crate>` with no `--target` prints "nothing to print" for target-gated deps. `quick-xml` looks absent on Windows because it enters via `wayland-scanner` (a `cfg(unix)` Wayland, build-time dep) — real, but never compiled on our Windows target. `--target all` reveals the true path. A crate reached only through a foreign-OS or build-time path changes the reachability verdict in Step 5.

## Step 4 — Choose the remediation (decision matrix)

| Situation | Action |
|---|---|
| Fixed version is **semver-compatible** with what's resolved (patch/minor within the same `^` range) | `cargo update -p <crate>` (or `--precise <ver>`). Lockfile-only, no manifest edit. Prefer this — it's free. |
| Vulnerable crate is a **direct dep** and fix needs a version bump | Bump the requirement in that crate's `Cargo.toml`, then `cargo update -p <crate>`. |
| Fix is a **semver-major** bump of a **transitive** crate, and a **parent pins the old range** (e.g. `wayland-scanner` requires `quick-xml ^0.39`, fix needs `>=0.41`) | Cannot `cargo update` across it. Try bumping the *parent* that pins it; if the parent has no update either, it is **blocked upstream** → go to Step 5. |

Confirm any bump with `cargo update -p <crate> --dry-run` before editing. After Step 4, re-run **both** gates.

## Step 5 — Only if genuinely unfixable: assess reachability, then defer (with consult)

Deferring is a judgment call. Per [CLAUDE.md](../../../CLAUDE.md) you must **not silence a gate to make it pass** — investigate and, for judgment calls, consult the user before editing config. Build the case first:

- **Reachability / threat model** — does our code path reach the vulnerable API with attacker-controlled input? (e.g. the `quick-xml` DoS advisories target untrusted network XML; `wayland-scanner` only parses local, trusted protocol XML at build time, and not on Windows → effectively unreachable here.)
- If the fix is reachable and available, **fix it — do not defer.**

To defer a confirmed-unreachable, upstream-blocked advisory you must add the ID to **both** ignore lists (they are separate — one alone still fails the other gate):

```toml
# deny.toml  [advisories]
ignore = [
    { id = "RUSTSEC-XXXX-YYYY", reason = "何経由か・なぜ到達不能か・解除トリガ（例: 上流が要件を上げたら）" },
]
```
```toml
# .cargo/audit.toml  [advisories]
ignore = [ "RUSTSEC-XXXX-YYYY" ]     # audit.toml reason はコメントで残す
```
Also add a bullet to CLAUDE.md's 「既知の非ブロッキング警告」 with the removal trigger, mirroring how `yaml-rust` is tracked. Re-audit when the blocking parent (`eframe`/`wayland-scanner` etc.) updates, and drop the ignore the moment an upgrade path appears.

## Step 6 — Verify green

```
just ci     # full gate: must exit 0 (deny AND audit both pass)
```
`just check` skips deny/audit — it does **not** prove advisory findings are resolved. Only `just ci` (or running both tools directly) does.

## Common mistakes

- Declaring "audit is just warnings, we're fine" without running `cargo deny` — misses `unsound`/direct-dep `unmaintained` which block CI via deny only.
- Running `cargo tree -i` without `--target all` on Windows and concluding a target-gated crate "isn't used".
- Blindly bumping a transitive crate's `Cargo.toml` requirement when the real blocker is a *parent* pinning the old range.
- Ignoring in only one of the two config files → the other gate still red.
- Reaching for `ignore` before checking `cargo update -p` — most patch-level advisories fix for free.
- Trusting CLAUDE.md's advisory examples as the current set. The advisory DB drifts (the `yaml-rust` note there predates today's `crossbeam-epoch`/`quick-xml`/`anyhow` findings). Re-run the tools; treat any list in docs as illustrative, not live.
