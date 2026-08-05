//! Rate limiting for the two write paths that mint state on someone else's
//! behalf: attestation ingest and share-link creation
//! (`docs/PRODUCTION_CHECKLIST.md` §4).
//!
//! ## Why only these two
//!
//! §4 names ingest and share-link creation specifically, and the scope is
//! deliberate. Reads are cheap and per-tenant scoped; the public share
//! redemption GET is the one path an auditor with no key must always be able
//! to hit, so throttling it would break the product's core promise at exactly
//! the wrong moment. Ingest writes blobs and rows; share-link creation mints
//! public read grants. Those are the paths where abuse costs us.
//!
//! ## Keying: the `x-api-key` header
//!
//! The goal §4 is really after is tenant isolation — one account must not be
//! able to exhaust another's budget. The natural key is the account UUID, but
//! it isn't reachable from a layer: resolving a key to an account is an
//! `async` Argon2 scan over stored hashes ([`crate::auth`]), and running it
//! here would mean hashing twice per request.
//!
//! So we key on the raw `x-api-key` header value instead. It is a faithful
//! stand-in for the account on every path that matters:
//!
//! - Both limited routes are `AuthedAccount`-gated, so a request that gets
//!   past the limiter with a bogus key is rejected downstream anyway.
//! - Distinct tenants send distinct keys, so budgets don't collide — the
//!   isolation property §4 asks for.
//! - One account holding several keys gets a bucket per key rather than one
//!   shared bucket. That is more generous than strict per-account limiting,
//!   not less, so it can't cause a tenant to be throttled by another's
//!   traffic.
//!
//! Requests with no `x-api-key` at all share a single bucket. They cannot
//! succeed on these routes regardless, and bounding them keeps an
//! unauthenticated flood from reaching the Argon2 scan.
//!
//! We deliberately do **not** key on IP: tenants behind one NAT would share a
//! budget, and the service sits behind a proxy where the peer address is the
//! proxy's.
//!
//! ## Bounding the keyspace
//!
//! Keying on an unvalidated header has a cost: the limiter runs before auth,
//! so a caller can mint a fresh bucket per request just by varying
//! `x-api-key`, and `governor`'s keyed store never evicts on its own. Left
//! alone, the layer we added to *bound* abuse would be an unbounded-memory
//! vector. [`RateLimitService::sweep_if_due`] therefore reclaims fully
//! refilled buckets on a fixed request cadence.
//!
//! ## Why `governor` directly rather than `tower_governor`
//!
//! `tower_governor` 0.8's `Service` impl requires
//! `S::Response: From<GovernorError>`, and the only impl it ships for that is
//! `Response<axum::body::Body>` behind its `axum` feature — which is **axum
//! 0.8**, while this service is on **0.7**. The two `Body` types are distinct,
//! so that impl can never apply here, and `http::Response` is foreign so we
//! can't write our own. Its default features also pull a full `tonic` gRPC
//! stack. `governor` *is* the engine `tower_governor` wraps, and the layer we
//! need is small, so we use it directly and keep full control of the 429 body.

use std::num::NonZeroU32;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use axum::http::Request;
use axum::response::IntoResponse;
use governor::clock::DefaultClock;
use governor::state::keyed::DefaultKeyedStateStore;
use governor::{Quota, RateLimiter};

use crate::error::ApiError;

/// Env var overriding the per-minute request allowance on the limited routes.
pub const RATE_LIMIT_ENV: &str = "ATTESTLY_RATELIMIT_PER_MIN";

/// Pilot default: requests per minute per API key, per limited route.
///
/// Sized to be invisible to a well-behaved SDK client — the LangGraph sink
/// posts one attestation per completed agent task — while still bounding a
/// runaway loop. Raise it via [`RATE_LIMIT_ENV`] rather than editing this.
pub const DEFAULT_PER_MINUTE: u32 = 120;

/// Bucket shared by all requests that arrive with no usable `x-api-key`.
/// They can't succeed on the limited routes anyway; this just bounds them.
const ANONYMOUS_BUCKET: &str = "<anonymous>";

type Limiter = RateLimiter<String, DefaultKeyedStateStore<String>, DefaultClock>;

/// Read the configured per-minute limit, falling back to
/// [`DEFAULT_PER_MINUTE`] when [`RATE_LIMIT_ENV`] is unset, unparseable, or
/// zero.
pub fn per_minute_from_env() -> u32 {
    parse_per_minute(std::env::var(RATE_LIMIT_ENV).ok().as_deref())
}

/// The parsing half of [`per_minute_from_env`], split out so it can be tested
/// without mutating process-wide environment state (which would race the rest
/// of the suite).
///
/// Zero is treated as "unset" on purpose: a misconfigured `0` that literally
/// meant "allow nothing" would take ingest down completely, which is a worse
/// failure than not limiting. Operators who want the limiter effectively off
/// should set a high number.
fn parse_per_minute(raw: Option<&str>) -> u32 {
    let Some(raw) = raw else {
        return DEFAULT_PER_MINUTE;
    };
    match raw.trim().parse::<u32>() {
        Ok(n) if n > 0 => n,
        _ => {
            tracing::warn!(
                var = RATE_LIMIT_ENV,
                value = %raw,
                default = DEFAULT_PER_MINUTE,
                "invalid rate limit, using default"
            );
            DEFAULT_PER_MINUTE
        }
    }
}

/// The bucket key for a request: its `x-api-key`, or a shared anonymous
/// bucket. See the module docs for why this stands in for the account id.
fn bucket_key<T>(req: &Request<T>) -> String {
    req.headers()
        .get("x-api-key")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .unwrap_or(ANONYMOUS_BUCKET)
        .to_string()
}

/// Build the limiter layer for the write routes at `per_minute` requests per
/// minute per key.
///
/// The bucket holds a full `per_minute` burst and refills continuously, so a
/// client that paces itself is never throttled while a burst past the
/// allowance is refused immediately. Rejections carry
/// [`ApiError::RateLimited`], so a 429 uses the same `{error, detail}`
/// envelope as every other error in `API.md`.
pub fn layer(per_minute: u32) -> RateLimitLayer {
    let per_minute = NonZeroU32::new(per_minute.max(1)).expect("max(1) is non-zero");
    // `with_period` + `allow_burst` rather than `Quota::per_minute`: it lets
    // the refill interval and the burst ceiling be set independently, so the
    // bucket refills one request's worth at a time instead of all at once on
    // a minute boundary.
    let period = Duration::from_secs(60) / per_minute.get();
    let quota = Quota::with_period(period)
        .expect("period is non-zero for per_minute >= 1")
        .allow_burst(per_minute);
    RateLimitLayer {
        limiter: Arc::new(RateLimiter::keyed(quota)),
        since_sweep: Arc::new(AtomicU32::new(0)),
    }
}

/// Requests between stale-bucket sweeps. See [`RateLimitService::call`].
///
/// A sweep is an O(live keys) pass over a `DashMap`, so it must not run per
/// request; every 1024 it is amortised to nothing while still bounding the map
/// long before it can threaten a 256MB Fly machine.
const SWEEP_EVERY: u32 = 1024;

/// Tower layer applying the per-key token bucket.
#[derive(Clone)]
pub struct RateLimitLayer {
    limiter: Arc<Limiter>,
    since_sweep: Arc<AtomicU32>,
}

impl<S> tower::Layer<S> for RateLimitLayer {
    type Service = RateLimitService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        RateLimitService {
            inner,
            limiter: self.limiter.clone(),
            since_sweep: self.since_sweep.clone(),
        }
    }
}

#[derive(Clone)]
pub struct RateLimitService<S> {
    inner: S,
    limiter: Arc<Limiter>,
    since_sweep: Arc<AtomicU32>,
}

impl<S> RateLimitService<S> {
    /// Drop bucket state that has become indistinguishable from a fresh one,
    /// once every [`SWEEP_EVERY`] requests.
    ///
    /// `governor`'s keyed store never evicts on its own — it keeps one entry
    /// per key it has ever seen, and its own docs flag this housekeeping as
    /// the caller's job. That matters here because the limiter runs *before*
    /// auth (deliberately — a flood must not reach the Argon2 scan), so the
    /// key is an unvalidated header: anyone can mint a new bucket per request
    /// by varying `x-api-key`. Without this, the layer added to bound abuse
    /// would itself be an unbounded-memory vector.
    ///
    /// Only buckets whose tokens have fully refilled are dropped, so a caller
    /// mid-burst never has their budget forgiven by a sweep.
    fn sweep_if_due(&self) {
        // Relaxed is right: the counter only paces an optimisation, so a
        // racing pair of requests landing on the same tick costs at most one
        // extra (or one skipped) sweep.
        let n = self.since_sweep.fetch_add(1, Ordering::Relaxed);
        if n >= SWEEP_EVERY {
            self.since_sweep.store(0, Ordering::Relaxed);
            self.limiter.retain_recent();
            self.limiter.shrink_to_fit();
        }
    }
}

impl<S, ReqBody> tower::Service<Request<ReqBody>> for RateLimitService<S>
where
    S: tower::Service<Request<ReqBody>, Response = axum::response::Response>,
    S::Future: Send + 'static,
{
    type Response = axum::response::Response;
    type Error = S::Error;
    type Future = std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Self::Response, Self::Error>> + Send>,
    >;

    fn poll_ready(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        self.sweep_if_due();
        let key = bucket_key(&req);
        if self.limiter.check_key(&key).is_err() {
            // Over budget: refuse without touching the handler, so a flood
            // never reaches the Argon2 scan or the blob store.
            tracing::debug!(route = %req.uri().path(), "rate limit exceeded");
            return Box::pin(async move { Ok(ApiError::RateLimited.into_response()) });
        }
        let fut = self.inner.call(req);
        Box::pin(fut)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;

    fn req(key: Option<&str>) -> Request<Body> {
        let mut b = Request::builder().uri("/v1/attestations");
        if let Some(k) = key {
            b = b.header("x-api-key", k);
        }
        b.body(Body::empty()).unwrap()
    }

    #[test]
    fn extracts_the_api_key_as_the_bucket() {
        assert_eq!(bucket_key(&req(Some("key-abc"))), "key-abc");
    }

    #[test]
    fn distinct_keys_get_distinct_buckets() {
        // The tenant-isolation property §4 is after.
        assert_ne!(
            bucket_key(&req(Some("tenant-a"))),
            bucket_key(&req(Some("tenant-b")))
        );
    }

    #[test]
    fn missing_or_blank_key_falls_back_to_the_shared_bucket() {
        assert_eq!(bucket_key(&req(None)), ANONYMOUS_BUCKET);
        assert_eq!(bucket_key(&req(Some("   "))), ANONYMOUS_BUCKET);
    }

    #[test]
    fn a_valid_env_limit_is_honoured() {
        assert_eq!(parse_per_minute(Some("500")), 500);
        assert_eq!(parse_per_minute(Some("  42 ")), 42);
    }

    #[test]
    fn unset_or_bogus_env_limit_falls_back_to_the_default() {
        for raw in [None, Some(""), Some("0"), Some("banana"), Some("-5")] {
            assert_eq!(
                parse_per_minute(raw),
                DEFAULT_PER_MINUTE,
                "{raw:?} must fall back to the default"
            );
        }
    }

    #[test]
    fn rate_limited_maps_to_429() {
        let resp = ApiError::RateLimited.into_response();
        assert_eq!(resp.status(), axum::http::StatusCode::TOO_MANY_REQUESTS);
    }

    /// A `tower::Service` that answers everything 200, so the tests below
    /// exercise the layer through its real `call` path.
    #[derive(Clone)]
    struct Ok200;

    impl tower::Service<Request<Body>> for Ok200 {
        type Response = axum::response::Response;
        type Error = std::convert::Infallible;
        type Future = std::future::Ready<Result<Self::Response, Self::Error>>;

        fn poll_ready(
            &mut self,
            _: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Result<(), Self::Error>> {
            std::task::Poll::Ready(Ok(()))
        }

        fn call(&mut self, _: Request<Body>) -> Self::Future {
            std::future::ready(Ok(axum::http::StatusCode::OK.into_response()))
        }
    }

    #[tokio::test]
    async fn stale_buckets_are_reclaimed_so_the_keyspace_stays_bounded() {
        use tower::{Layer, Service};

        // `governor` never evicts on its own, and the limiter keys on an
        // unvalidated header *before* auth runs — so without the sweep, an
        // attacker varying `x-api-key` grows the map without bound. A high
        // limit keeps the refill period short, so every bucket a request
        // touches is fully refilled (and therefore reclaimable) almost
        // immediately; the point under test is that the sweep runs at all.
        let l = layer(60_000);
        let mut svc = l.layer(Ok200);

        let n = SWEEP_EVERY * 2;
        for i in 0..n {
            let req = Request::builder()
                .uri("/v1/attestations")
                .header("x-api-key", format!("bogus-{i}"))
                .body(Body::empty())
                .unwrap();
            let _ = svc.call(req).await.unwrap();
        }

        assert!(
            l.limiter.len() < n as usize,
            "distinct-key buckets must be reclaimed, not retained forever \
             (held {} of {n} after {} requests)",
            l.limiter.len(),
            n
        );
    }

    #[test]
    fn a_sweep_does_not_forgive_a_caller_mid_burst() {
        // The sweep must only drop buckets that have fully refilled. If it
        // dropped an in-use one, an attacker could reset their own budget by
        // flooding distinct keys until a sweep fired.
        let l = layer(2);
        let key = "victim".to_string();
        assert!(l.limiter.check_key(&key).is_ok());
        assert!(l.limiter.check_key(&key).is_ok());
        assert!(l.limiter.check_key(&key).is_err(), "budget spent");

        l.limiter.retain_recent();
        l.limiter.shrink_to_fit();

        assert!(
            l.limiter.check_key(&key).is_err(),
            "a sweep must not hand back an exhausted budget"
        );
    }

    #[test]
    fn the_bucket_admits_a_full_burst_then_refuses() {
        // Unit-level proof of the limiter itself; `tests/rate_limit.rs` proves
        // the same through the router.
        let l = layer(3);
        for i in 0..3 {
            assert!(l.limiter.check_key(&"k".to_string()).is_ok(), "request {i}");
        }
        assert!(l.limiter.check_key(&"k".to_string()).is_err());
        // A different key has its own budget — tenant isolation.
        assert!(l.limiter.check_key(&"other".to_string()).is_ok());
    }
}
