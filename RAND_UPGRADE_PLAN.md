# rand 0.10 Upgrade Plan

## Dependency Chain

```
workspace rand 0.8 ──> 0.10
  ├── identity
  │     ├── k256 0.13 ──> elliptic-curve 0.13 ──> rand_core 0.6
  │     ├── p256 0.13 ──> elliptic-curve 0.13 ──> rand_core 0.6
  │     └── ed25519-dalek 2.2 ──> curve25519-dalek 4.1 ──> rand_core 0.6
  └── transports/noise
        ├── snow 0.9 ──> rand_core 0.6
        └── x25519-dalek 2 ──> curve25519-dalek 4 ──> rand_core 0.6
```

## Phase 1 — Bump the crypto stack (rand_core 0.6 → 0.10)

| Step | Crate | Current → Target | Key changes beyond rand |
|------|-------|------------------|------------------------|
| 1 | `elliptic-curve` | 0.13.8 → **0.14.1** | Uses `crypto-common`, `hybrid-array` |
| 2 | `k256` | 0.13.4 → **0.14.0** | Needs `elliptic-curve ^0.14`, may use `sha2 ^0.11` |
| 3 | `p256` | 0.13.2 → **0.14.0** | Same as k256 |
| 4 | `curve25519-dalek` | 4.1.3 → **5.0.0** | Uses `subtle ^2.6` |
| 5 | `x25519-dalek` | 2.0.1 → **3.0.0** | Needs `curve25519-dalek ^5`, `rand_core ^0.10` |
| 6 | `ed25519-dalek` | 2.2.0 → **3.0.0** | Needs `curve25519-dalek ^5`, `ed25519 ^3` |
| 7 | `snow` | 0.9.6 → **0.10.0** | Builder returns `Result` instead of `Builder` |

Steps 1–3 can be done together. Steps 4–6 are a second group (via curve25519-dalek). Step 7 is independent.

## Phase 2 — Update `identity` (published separately, own versioning)

```toml
ed25519-dalek = "2.1"  →  "3"
k256 = "0.13"           →  "0.14"
p256 = "0.13"           →  "0.14"
sha2 = "0.10"           →  "0.11"   # optional, may be pulled by k256/p256
```

`identity` uses its own edition/versioning (does **not** inherit from workspace), so this needs a semver bump of `libp2p-identity`.

## Phase 3 — Update `transports/noise`

```toml
snow = "0.9"            →  "0.10"
x25519-dalek = "2"      →  "3"
```

Also fix the builder API: `.prologue()`, `.local_private_key()` now return `Result<Builder>` instead of `Builder`.

## Phase 4 — Update workspace root `Cargo.toml`

```toml
rand = "0.8"            →  "0.10"
getrandom = "0.2"       →  "0.3"    # snow 0.10 needs ^0.3
```

`getrandom` 0.3 supports wasm via `features = ["js"]` — same as 0.2.

## Phase 5 — Fix rand API usage across workspace crates

~20 crates use `rand = { workspace = true }` and need source-level fixes:

| Old API (rand 0.8) | New API (rand 0.10) |
|---------------------|---------------------|
| `rand::thread_rng()` | `rand::rng()` |
| `rand::thread_rng().gen::<T>()` | `rand::random::<T>()` |
| `rand::thread_rng().gen_range(a..b)` | `rand::random_range(a..b)` |
| `rand::thread_rng().fill_bytes(&mut x)` | `rand::fill(&mut x)` |
| `rand::thread_rng().next_u64()` | `rand::random::<u64>()` |
| `rand::thread_rng().sample(d)` | `rand::rng().sample(d)` (needs `RngExt`) |
| `rand::thread_rng().choose(s)` / `.choose_multiple(s, n)` | `rand::rng().choose(s)` (needs `RngExt`) |
| `use rand::Rng` (trait for `gen`, `gen_range`, etc.) | `use rand::RngExt` |
| `use rand::RngCore` | `use rand::Rng` |
| `use rand::distributions::{Standard, Alphanumeric}` | `use rand::distr::{StandardUniform, Alphanumeric}` |
| `use rand::seq::SliceRandom` | `use rand::seq::IndexedRandom` |

## Phase 6 — Cleanup autonat

```toml
# protocols/autonat/Cargo.toml
rand_core = { version = "0.6", optional = true }  →  { workspace = true }
```

## Compatibility notes

- **webrtc** (0.17.0) already uses `rand ^0.9` — no update needed. rand 0.9 and 0.10 will coexist in `Cargo.lock`.
- **salsa20** (0.10.2 → 0.11.0) has no rand dep — can be done independently.
- **sha2, sha3, hkdf** also have no rand dep — bumps can be done independently.

## Recommended execution order

| PR | Scope | Risk |
|----|-------|------|
| **PR 1** | Bump crypto stack (elliptic-curve, k256, p256) + update `identity` | High — semver breaking in identity |
| **PR 2** | Bump curve25519-dalek, x25519-dalek, ed25519-dalek | Medium |
| **PR 3** | Bump snow to 0.10 + fix noise builder API | Medium |
| **PR 4** | Bump workspace rand to 0.10 + bulk fix all crate rand usage | High — touches 20+ files |
| **PR 5** | Bump getrandom to 0.3 + cleanup autonat rand_core | Low |
