
// Slimechain algorithm Rust implementation
// - Comments are written in English
// - Composed of pure functions with no external state

use serde::{Deserialize, Serialize};

/// Parameter bundle
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Params {
    pub q_weights: QWeights,
    pub q_min: f64,
    pub ef: EfParams,
    pub cost: CostParams,
    pub propagation: PropagationParams,
    pub reward: RewardParams,
    pub congestion: CongestionParams,
}

impl Default for Params {
    fn default() -> Self {
        Self {
            q_weights: QWeights::default(),
            q_min: 0.5,
            ef: EfParams { gamma: 0.8, cap: 10.0 },
            cost: CostParams {
                alpha: 0.7, beta: 0.5, a: 1.2, b: 0.6,
                lambda_actor: 0.6, lambda_content: 0.4,
                rate_limit_per_hour: 10.0,
            },
            propagation: PropagationParams { ttl_base: 4.0, fanout_base: 5.0, k1: 2.0, k2: 2.0 },
            reward: RewardParams { r0: 1.0, mu: 0.3 },
            congestion: CongestionParams { eta: 0.1, target_load: 500.0, base_min: 0.1, base_max: 100.0 },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QWeights { pub w_a: f64, pub w_r: f64, pub w_t: f64, pub w_d: f64, pub w_h: f64, pub w_s: f64 }
impl Default for QWeights {
    fn default() -> Self { Self{ w_a:0.2, w_r:0.2, w_t:0.2, w_d:0.15, w_h:0.2, w_s:0.25 } }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EfParams { pub gamma: f64, pub cap: f64 }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostParams {
    pub alpha: f64, pub beta: f64, pub a: f64, pub b: f64,
    pub lambda_actor: f64, pub lambda_content: f64,
    pub rate_limit_per_hour: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropagationParams { pub ttl_base: f64, pub fanout_base: f64, pub k1: f64, pub k2: f64 }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RewardParams { pub r0: f64, pub mu: f64 }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CongestionParams { pub eta: f64, pub target_load: f64, pub base_min: f64, pub base_max: f64 }

/// Quality score inputs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QInputs { pub A: f64, pub R: f64, pub T: f64, pub D: f64, pub H: f64, pub S: f64 }

/// Actor (author) input
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Actor {
    /// Recent average request load (keep unit definition consistent, e.g., per minute)
    pub rl: f64,
    /// Quality score
    pub q: f64,
    /// Effective followers
    pub ef: f64,
    /// Posts in the last hour (used for rate-limit penalty)
    pub posts_1h: Option<f64>,
}

/// Content input (factual claim/evidence and risk signals)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Content {
    pub is_claim: Option<bool>,
    pub has_evidence: Option<bool>,
    pub risk_signals: Option<RiskSignals>,
}

/// Risk signals (0..1)
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RiskSignals {
    pub coordination: Option<f64>,
    pub clustering: Option<f64>,
    pub burst: Option<f64>,
    pub monotonicity: Option<f64>,
    pub abuse_history: Option<f64>,
}

/// Risk weights
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskWeights { pub w_coord: f64, pub w_clust: f64, pub w_burst: f64, pub w_mono: f64, pub w_hist: f64 }
impl Default for RiskWeights {
    fn default() -> Self { Self{ w_coord:0.25, w_clust:0.25, w_burst:0.20, w_mono:0.15, w_hist:0.15 } }
}

/// Propagation result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropagationResult { pub ttl: u32, pub fanout: u32 }

/// Reward calculation input
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RewardInput {
    pub ticket_budget: f64,
    pub client_q: f64,
    pub size_bytes: u64,
    pub ttfb_ms: u32,
    pub server_cluster_risk: f64,
}

// -------- Utilities --------

fn clamp(x: f64, lo: f64, hi: f64) -> f64 { x.max(lo).min(hi) }

fn v(opt: Option<f64>) -> f64 { opt.unwrap_or(0.0) }

// -------- Quality/EF --------

/// Compute quality score q
pub fn calculate_quality(inp: QInputs, params: &Params) -> f64 {
    let w = &params.q_weights;
    let mut q = w.w_a*inp.A + w.w_r*inp.R + w.w_t*inp.T + w.w_d*inp.D + w.w_h*inp.H - w.w_s*inp.S;
    q = clamp(q, 0.0, 1.0);
    if inp.H == 0.0 { q = q.min(0.4); } // TG unverified cap
    q
}

/// Compute effective followers EF
pub fn calculate_ef(followers_q: &[f64], params: &Params) -> f64 {
    let gamma = params.ef.gamma;
    let cap = params.ef.cap;
    let mut sum = 0.0;
    for &q in followers_q {
        if q >= params.q_min { sum += q.powf(gamma); }
    }
    sum.ln_1p() * cap
}

// -------- Risk --------

/// Compute risk score (0..1)
pub fn calculate_risk(signals: &Option<RiskSignals>, weights: &RiskWeights) -> f64 {
    let s = signals.as_ref().cloned().unwrap_or_default();
    let r = weights.w_coord*v(s.coordination)
          + weights.w_clust*v(s.clustering)
          + weights.w_burst*v(s.burst)
          + weights.w_mono*v(s.monotonicity)
          + weights.w_hist*v(s.abuse_history);
    clamp(r, 0.0, 1.0)
}

// -------- Posting cost (DPP) --------

/// Compute posting cost
pub fn calculate_post_cost(actor: &Actor, content: &Content, params: &Params, base_fare: f64) -> f64 {
    let a = params.cost.a;
    let b = params.cost.b;
    let alpha = params.cost.alpha;
    let beta = params.cost.beta;
    let lambda_a = params.cost.lambda_actor;
    let lambda_c = params.cost.lambda_content;

    let rl_cost = a * actor.rl.max(0.0).powf(alpha);
    let ef_cost = b * actor.ef.max(0.0).powf(beta);
    let mut cost = base_fare + rl_cost + ef_cost;

    let weights = RiskWeights::default();
    let risk_actor = calculate_risk(&content.risk_signals, &weights);
    let risk_content = calculate_risk(&content.risk_signals, &weights);
    cost *= 1.0 + lambda_a*risk_actor + lambda_c*risk_content;

    if content.is_claim.unwrap_or(false) {
        if content.has_evidence.unwrap_or(false) { cost *= 0.7; }
        else { cost *= 1.2; }
    }

    if let Some(posts) = actor.posts_1h {
        let rate = params.cost.rate_limit_per_hour.max(1.0);
        if posts > rate {
            let over = posts / rate - 1.0;
            cost *= 1.0 + 0.5 * over;
        }
    }
    cost
}

// -------- Propagation control (RWP/TFR) --------

/// Adjust TTL/Fanout
pub fn adjust_propagation(risk_signals: &Option<RiskSignals>, params: &Params) -> PropagationResult {
    let weights = RiskWeights::default();
    let risk = calculate_risk(risk_signals, &weights);
    let ttl = clamp(params.propagation.ttl_base - params.propagation.k1 * risk, 1.0, params.propagation.ttl_base);
    let fanout = clamp(params.propagation.fanout_base - params.propagation.k2 * risk, 1.0, params.propagation.fanout_base);
    PropagationResult { ttl: ttl.round() as u32, fanout: fanout.round() as u32 }
}

// -------- PoR/S reward --------

/// Compute serving reward
pub fn calculate_serve_reward(input: &RewardInput, params: &Params) -> f64 {
    let r0 = params.reward.r0;
    let mu = params.reward.mu;
    let w_size = (1.0 + (input.size_bytes as f64)).ln() / (1.0 + 1_000_000.0_f64).ln();
    let w_latency = 1.0 / (1.0 + (input.ttfb_ms as f64) / 1000.0);
    let diversity = 1.0 - mu * clamp(input.server_cluster_risk, 0.0, 1.0);
    let reward = r0 * clamp(input.client_q, 0.0, 1.0) * w_size * w_latency * diversity;
    reward.min(input.ticket_budget.max(0.0))
}

// -------- Congestion control base fare --------

/// Update base fare
pub fn update_base_cost(current_base: f64, current_load: f64, params: &Params) -> f64 {
    let eta = params.congestion.eta;
    let target = params.congestion.target_load.max(1e-9);
    let mut b = current_base * ((eta * (current_load / target - 1.0))).exp();
    b = clamp(b, params.congestion.base_min, params.congestion.base_max);
    b
}

// -------- Tests (basic) --------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_quality_ef() {
        let params = Params::default();
        let q = calculate_quality(QInputs{ A:0.8, R:0.7, T:0.6, D:0.5, H:1.0, S:0.2 }, &params);
        assert!(q >= 0.0 && q <= 1.0);
        let ef = calculate_ef(&[0.8,0.7,0.4,0.9], &params);
        assert!(ef > 0.0);
    }

    #[test]
    fn test_cost_prop_reward() {
        let params = Params::default();
        let actor = Actor { rl:120.0, q:0.8, ef:30.0, posts_1h:Some(12.0) };
        let content = Content { is_claim:Some(true), has_evidence:Some(false), risk_signals:Some(RiskSignals{ coordination:Some(0.5), clustering:Some(0.4), burst:None, monotonicity:None, abuse_history:None }) };
        let cost = calculate_post_cost(&actor, &content, &params, 1.0);
        assert!(cost > 0.0);

        let pr = adjust_propagation(&content.risk_signals, &params);
        assert!(pr.ttl >= 1 && pr.ttl <= params.propagation.ttl_base as u32);

        let ri = RewardInput{ ticket_budget:1.5, client_q:0.8, size_bytes:24000, ttfb_ms:120, server_cluster_risk:0.2 };
        let rew = calculate_serve_reward(&ri, &params);
        assert!(rew >= 0.0);
    }

    #[test]
    fn test_base() {
        let params = Params::default();
        let b2 = update_base_cost(1.0, 1000.0, &params);
        assert!(b2 > 1.0);
    }
}
