---
name: analyzing-startup-profile
description: Use when analyzing startup-latency profiling output from `just profile-startup` (target/profile/trace.json, tracing.folded, flame.svg), interpreting tracing-chrome / tracing-flame results for the tcyb Twitch bot, or turning a startup trace into improvement proposals. Symptoms — reading a Chrome trace, span durations/counts look surprising, deciding what a timeline gap means, or proposing fixes from profile numbers.
---

# Analyzing the startup profile

## Overview

`just profile-startup` runs `tcyb read-chat` with the `profiling` feature and writes:
- `target/profile/trace.json` — Chrome trace (tracing-chrome), µs, `B`/`E` events. **Source of truth for wall-clock.**
- `target/profile/tracing.folded` + `flame.svg` — flamegraph. **Misleading for async** (see traps).

Instrumented spans: `config_build`, `logger_init`, `store_new`, `user_id_fetch`, `irc_connect`, `irc_auth`, `event_connect`, `event_subscribe`, `token_refresh`. "ready" = both IRC and EventSub marked ready ([profiling.rs](../../../tcyb/src/profiling.rs)).

**Core principle: the Chrome trace timeline is truth; span counts and folded self-time lie for async. Always cross-check the run's console log before drawing conclusions.**

## Step 1 — Establish what the run actually did (read the log FIRST)

The startup path is **conditional**. Determine which path this trace is before reading numbers:
- **fresh token** → clean path, **~0.65 s**.
- **stale token** → 401 on first subscribe → **one** refresh → reconnect, **~1.6 s (~3×)**. This is the lazy-refresh design (refresh only on connection error), not a bug.

Detect the path — the profiling run's console log is stdout and usually not persisted, so detect from the **trace itself**: a `token_refresh` span present (and `event_subscribe` happening twice) = stale-token; absent = fresh. If you do have the console log, a `401 Unauthorized` confirms it. Never report a stale-token run as "the startup cost."

## Step 2 — Parse the trace

Run the helper (don't eyeball folded numbers):
```
just analyze-trace          # defaults to target/profile/trace.json
just analyze-trace <path>   # explicit path
```
It pairs `B`/`E` per tid, prints per-span window/active time, the timeline, and **no-span gaps** (the waits). Source: [xtask/src/main.rs](../../../xtask/src/main.rs).

## Step 3 — Interpret (this is where naive analysis fails)

| Trap | Reality |
|---|---|
| "span appears N times → N operations / a loop" | `.instrument()` re-enters on **every poll** across await points. 9 `token_refresh` spans = **one** refresh polled 9×. Count operations from the **LOG**, never from span instance counts. |
| "folded value / span active-time = wall-clock" | For async the await wait falls **outside** the span (between poll slices). Self-time excludes I/O wait. Use the span's start→end window, or the no-span gaps, for wall-clock. |
| "the 1.5 s run is the startup cost" | Path is conditional. Compare against a clean (fresh-token) run before concluding. |
| "a big gap = our slow code" | Classify it (Step 4). Most gaps are Twitch server RTT, not our CPU. |

## Step 4 — Classify every gap

Each no-span gap is one of:
- **Twitch server RTT** — waiting for `session_welcome` after connect, or the subscribe/refresh HTTP response. *Unavoidable.*
- **Uninstrumented blind spot** — code with no span (the gap is total mystery). *Fix by adding a span,* then re-profile.
- **Backoff sleep** — `refresh_tokens_with_backoff` constants in [yomiage.rs](../../../tcyb/src/yomiage.rs). *Config, not latency.*
- **Local CPU** — rare here; startup is I/O-bound.

Cross-check each gap against the log timestamps and the source line.

## Step 5 — Ground EVERY proposal in the actual stack/source

Verify before proposing — the baseline failure mode is confident, ungrounded fixes:
- **TLS backend is native-tls** (`reqwest` + `tokio-tungstenite` use `features=["native-tls"]` in [tcyb/Cargo.toml](../../../tcyb/Cargo.toml)) → Windows schannel. **Do NOT propose rustls-specific fixes.**
- **eventsub/irc WebSocket connects go through tokio-tungstenite, NOT reqwest** → a shared reqwest client / pool does not touch those connects.
- HTTP API calls ([api.rs](../../../tcyb/src/api.rs)) already share one `HTTP_CLIENT` (lazy_static).
- Token refresh is **lazy by design** — don't propose "stop refreshing"; the right question is "was the token stale?"

Tag each proposal: **helps measured startup path** / **helps runtime or retry only** / **no startup effect**. Quantify with trace numbers.

## Quick reference (known baselines)

- clean ~0.65 s; stale-token ~1.6–1.75 s.
- sync prelude (config/logger/store/user_id) ≈ 1–13 ms total → **negligible; do not optimize it.**
- dominant cost = WS connect + Twitch RTT (welcome/subscribe); on stale token, + the refresh→reconnect detour (~0.8–1.1 s, conditional).

## Common mistakes (observed in real baseline analysis)

- Reading span instance count as operation count → inventing a "reconnect loop" that doesn't exist.
- Treating one conditional stale-token run as the permanent startup cost.
- Proposing rustls / pool fixes without checking the stack (it's native-tls; WS isn't reqwest).
- Optimizing the ~1 ms sync prelude.
