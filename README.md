# slimechain-algo (Rust) — Core Algorithms

**SlimeChain** is an *edge-first, demand-priced* social overlay.  
This crate implements the **pure, stateless math** only — the parts you can embed in any service (overlay node, GUI, indexer, or chain module) without bringing in networking or storage.

> All functions are deterministic and side‑effect free. Inputs are validated by clamping to safe ranges and using stable transforms (`max(0,·)`, `min(·)`, `log1p`), so production callers won’t get NaNs or panics from bad or adversarial inputs.

## What’s Implemented

1. **Quality & Effective Followers**
   - `calculate_quality(QInputs, Params) -> q in [0,1]`
   - `calculate_ef(&[q_follower], Params) -> EF >= 0`
2. **Risk Aggregation**
   - `calculate_risk(Option<RiskSignals>, RiskWeights) -> risk in [0,1]`
3. **Demand‑Priced Posting (DPP)**
   - `calculate_post_cost(actor, content, params, base_fare) -> cost >= 0`
4. **Risk‑Weighted Propagation (RWP/TFR)**
   - `adjust_propagation(risk_signals, params) -> { ttl, fanout }`
5. **Proof‑of‑Relay/Settlement (PoR/S) Reward**
   - `calculate_serve_reward(input, params) -> reward >= 0`
6. **Congestion‑Controlled Basefare (CCB)**
   - `update_base_cost(current_base, current_load, params) -> new_base`

The crate also exposes **`Params::default()`** and `RiskWeights::default()` with sane starting values to keep behavior understandable during early prototyping.

---

## Type Sketch

```rust
pub struct Params {
  pub q_weights: QWeights,         // quality weights
  pub q_min: f64,                  // EF inclusion threshold
  pub ef: EfParams,                // { gamma, cap }
  pub cost: CostParams,            // { alpha, beta, a, b, lambda_actor, lambda_content, rate_limit_per_hour }
  pub propagation: PropagationParams, // { ttl_base, fanout_base, k1, k2 }
  pub reward: RewardParams,           // { r0, mu }
  pub congestion: CongestionParams,   // { eta, target_load, base_min, base_max }
}

pub struct Actor { pub rl: f64, pub q: f64, pub ef: f64, pub posts_1h: Option<f64> }
pub struct Content { pub is_claim: Option<bool>, pub has_evidence: Option<bool>, pub risk_signals: Option<RiskSignals> }
```

---

## Formal Definitions (ASCII)

### 1) Quality `q` and Effective Followers `EF`

- **Quality**:
```
q = clamp( wA*A + wR*R + wT*T + wD*D + wH*H - wS*S , 0, 1 )
if H == 0 then q = min(q, 0.4)   // handshake-gate cap
```
Where:
- `A`=account longevity/activity, `R`=reciprocity, `T`=triadic closure, `D`=audience diversity, `H`=handshake flag (0/1), `S`=Sybil suspicion.

- **Effective Followers** (diminishing returns, quality‑filtered):
```
EF_raw = sum( q_f^gamma for q_f in followers if q_f >= q_min )
EF     = log(1 + EF_raw) * cap
```
**Monotonicity**: increasing any included follower’s `q_f` increases `EF`; adding low‑quality followers below `q_min` does not.

**Edge cases**:
- Missing/negative `q_f` → treated as 0 (excluded).
- `gamma <= 0` is nonsensical; library ships with gamma>0. Pass a sane value.
- `cap` scales EF to your economic domain; EF is real‑valued (not integer).

### 2) Risk Aggregation `risk`
```
risk = clamp( wCoord*Coord + wClust*Clust + wBurst*Burst + wMono*Mono + wHist*Hist , 0, 1 )
```
Unspecified signals default to 0. Use custom weights for different threat models.

### 3) Demand‑Priced Posting `C_post`
```
C_post = B_t + a * max(RL,0)^alpha + b * max(EF,0)^beta
C_post *= ( 1 + lambda_actor * Risk_actor + lambda_content * Risk_content )
if is_claim:
    if has_evidence: C_post *= 0.7
    else:            C_post *= 1.2
if posts_1h > rate_limit_per_hour:
    over = posts_1h / rate_limit_per_hour - 1
    C_post *= ( 1 + 0.5 * over )
```
Notes:
- `B_t` is the *current basefare* (from CCB below).
- `RL` = recent request‑load; **negative RL is truncated to 0**.
- `EF` is precomputed; library does not infer the follower graph.
- **No hard blocks**: you can still post with small `B_t` and low `EF/RL`; risk only *scales price* and *modulates propagation*.

### 4) Risk‑Weighted Propagation (TTL/Fanout)
```
ttl    = clamp( TTL_base   - k1 * risk , 1, TTL_base )
fanout = clamp( fanout_base - k2 * risk , 1, fanout_base )
```
Returned as rounded integers. Clamping guarantees a *non‑zero* path even for high risk.

### 5) PoR/S Serve Reward
```
w_size     = log(1 + size_bytes) / log(1 + 1_000_000)    // normalized ~[0,1]
w_latency  = 1 / (1 + ttfb_ms/1000)                       // faster → higher
diversity  = 1 - mu * clamp(clusterRisk, 0, 1)            // penalize server clusters

reward = r0 * clamp(clientQ,0,1) * w_size * w_latency * diversity
reward = min( reward, max(ticketBudget, 0) )
```
**Auditability**: submit receipts with the ticket nonce; reject duplicates; random re‑requests catch collusion.

### 6) Congestion‑Controlled Basefare `B`
```
B_next = clamp( B * exp( eta * ( Load / Target - 1 ) ) , base_min, base_max )
```
- Load/Target tune responsiveness. `exp` yields smooth, multiplicative adjustments.
- `base_min > 0` avoids “zero price” spiral; `base_max` keeps cost humane.

---

## Edge Cases & Invariants

| Function | Input Case | Handling | Result/Guarantee |
|---|---|---|---|
| `calculate_quality` | `H=0` | hard cap `q<=0.4` | prevents unverified influence |
|  | any component outside [0,1] | clamped implicitly via weights+clamp | `q in [0,1]` |
| `calculate_ef` | follower `q_f < q_min` | ignored | no EF inflation from low‑q |
|  | empty list | `EF=0` | graceful |
|  | extreme list (1e6 items) | O(n) pass | stable `log1p` prevents overflow |
| `calculate_risk` | missing fields | treated as 0 | `risk in [0,1]` |
| `calculate_post_cost` | `RL<0` or `EF<0` | `max(·,0)` | monotone non‑decreasing |
|  | `is_claim=None` | treated as false | no claim multiplier |
|  | `has_evidence=None` | ignored unless `is_claim=true` |  |
|  | `posts_1h=None` | no rate penalty |  |
|  | pathological values (NaN, inf) | caller should pre‑sanitize; clamping mitigates most cases | no panic in practice |
| `adjust_propagation` | `risk>1`/`<0` | clamped | `ttl>=1`, `fanout>=1` |
|  | fractional outputs | rounded to `u32` | UI‑friendly |
| `calculate_serve_reward` | `size_bytes<0`, `ttfb_ms<0` | `max(·,0)` | safe weights |
|  | `ticket_budget<0` | replaced by 0 cap | no negative payouts |
| `update_base_cost` | `target_load<=0` | guarded to `>=1e-9` | no div‑by‑zero |
|  | very high load | exponential damping + `base_max` | bounded |

**Invariants** (with sane params):  
- `∂C_post/∂RL >= 0`, `∂C_post/∂EF >= 0`.  
- Risk monotonicity: higher risk never increases ttl/fanout or decreases price.  
- Reward upper‑bounded by ticket budget; non‑negative for all inputs.

---

## Parameter Defaults (can/should be tuned)

```rust
Params::default() =>
  q_min=0.5, ef.gamma=0.8, ef.cap=10.0
  cost: alpha=0.7, beta=0.5, a=1.2, b=0.6, lambda_actor=0.6, lambda_content=0.4, rate_limit_per_hour=10
  propagation: ttl_base=4, fanout_base=5, k1=2.0, k2=2.0
  reward: r0=1.0, mu=0.3
  congestion: eta=0.1, target_load=500, base_min=0.1, base_max=100.0
```

**Tuning tips**:
- Increase `alpha` when high‑RL actors should pay sharply more.
- Decrease `beta` to keep hub posting viable while still pricier than edge.
- Increase `k1/k2` to damp risky spread; deploy canaries before governance changes.
- Increase `mu` to punish server collusion more strongly.
- Raise `eta` to react faster to surges (at cost of price volatility).

---

## CLI

```bash
# Build
cargo build --release

# Cost
./target/release/slimechain-algo cost examples/cost-input.json

# Reward
./target/release/slimechain-algo reward examples/reward-input.json

# Propagation (ttl/fanout)
./target/release/slimechain-algo prop examples/prop-input.json

# Basefare update
./target/release/slimechain-algo base examples/base-input.json

# Quality
./target/release/slimechain-algo quality examples/quality-input.json

# EF
./target/release/slimechain-algo ef examples/ef-input.json

# Risk
./target/release/slimechain-algo risk examples/risk-input.json
```

### JSON Shapes (informal)

- **Cost** (`cost-input.json`)
```json
{
  "actor": { "rl": 120.0, "q": 0.82, "ef": 28.3, "posts_1h": 12.0 },
  "content": { "is_claim": true, "has_evidence": false, "risk_signals": { "coordination": 0.5, "clustering": 0.4 } },
  "base_fare": 1.0
}
```
- **Reward** (`reward-input.json`)
```json
{ "ticket_budget": 1.5, "client_q": 0.8, "size_bytes": 25000, "ttfb_ms": 150, "server_cluster_risk": 0.3 }
```
- **Propagation** (`prop-input.json`)
```json
{ "risk_signals": { "coordination": 0.8, "clustering": 0.7 } }
```
- **Basefare** (`base-input.json`)
```json
{ "current_base": 1.0, "current_load": 1000.0 }
```
- **Quality** (`quality-input.json`)
```json
{ "A": 0.8, "R": 0.7, "T": 0.6, "D": 0.5, "H": 1.0, "S": 0.2 }
```
- **EF** (`ef-input.json`)
```json
[0.8, 0.7, 0.4, 0.9]
```
- **Risk** (`risk-input.json`)
```json
{ "coordination": 0.6, "clustering": 0.5, "burst": 0.3, "monotonicity": 0.2, "abuse_history": 0.1 }
```

---

## Examples (quick sanity)

```rust
use slimechain_algo::*;

let p = Params::default();

// Quality & EF
let q = calculate_quality(QInputs{ A:0.8, R:0.7, T:0.6, D:0.5, H:1.0, S:0.2 }, &p);
let ef = calculate_ef(&[0.8,0.7,0.4,0.9], &p);

// Cost
let actor = Actor{ rl:120.0, q, ef, posts_1h:Some(12.0) };
let content = Content{ is_claim:Some(true), has_evidence:Some(false),
                       risk_signals:Some(RiskSignals{ coordination:Some(0.5), clustering:Some(0.4), ..Default::default() }) };
let cost = calculate_post_cost(&actor, &content, &p, 1.0);

// Propagation
let pr = adjust_propagation(&content.risk_signals, &p);

// Reward
let ri = RewardInput{ ticket_budget:1.5, client_q:0.8, size_bytes:24000, ttfb_ms:120, server_cluster_risk:0.2 };
let reward = calculate_serve_reward(&ri, &p);

// Basefare
let b2 = update_base_cost(1.0, 1000.0, &p);
```

---

## Performance & Determinism

- All functions are *O(n)* or *O(1)* with no heap allocations beyond iterating inputs.
- Floating‑point math uses `f64`; results are deterministic on the same platform/inputs.
- Use your own RNG for audits; this crate intentionally includes **no randomness**.

---

## Testing

- Unit tests cover sanity (`cargo test`).
- We recommend you add property tests:
  - Monotonicity: `RL↑ ⇒ cost↑`, `EF↑ ⇒ cost↑`, `risk↑ ⇒ ttl↓, fanout↓`.
  - Boundedness: all outputs within described ranges.
  - Idempotence: repeated calls with same inputs are equal.

---

## Integration Patterns

- **Overlay node**: Call cost/propagation at publish time; cache EF per author periodically.
- **Chain module**: Use reward and basefare; validate receipts; anchor parameter changes.
- **GUI**: Show *cost preview* and *reach estimates* from ttl/fanout, not as promises but ranges.

---

## Security & Abuse Considerations

- Treat all external numbers as adversarial; the crate clamps everything, but you should **authenticate** the source and rate‑limit upstream.
- Separate *actor risk* and *content risk* in your app if you have richer signals (pass different `risk_signals` objects to each multiplier).

---

## License

- Code: MIT
- Documentation: CC BY‑SA 4.0
