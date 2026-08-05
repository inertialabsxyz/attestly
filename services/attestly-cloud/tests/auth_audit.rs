//! Auth audit — the standing guard on the service's public/private boundary.
//!
//! `docs/PRODUCTION_CHECKLIST.md` §4 requires that every `/v1/*` data route
//! demands a valid API key, with no unauthenticated data leakage. Auth here is
//! an *extractor*, not middleware ([`attestly_cloud::auth::AuthedAccount`]):
//! a route is protected **iff its handler takes an `AuthedAccount` param**.
//! That makes the boundary easy to break by accident — deleting one parameter
//! silently opens a route, and there is no layer stack that would notice.
//!
//! So this file pins the boundary from the outside, as a table. Every route is
//! one row in either [`PROTECTED`] or [`PUBLIC`]:
//!
//! - [`PROTECTED`] rows must return `401 unauthorized` with **no** `x-api-key`
//!   *and* with a **wrong** key.
//! - [`PUBLIC`] rows must be reachable **without** a key — i.e. they must not
//!   answer `401`. They are public by deliberate contract (documented per row),
//!   not by oversight.
//!
//! Adding a route to `lib.rs` without adding a row here is the failure mode
//! this test exists to catch, so keep the tables exhaustive:
//! [`route_tables_cover_every_route`] counts the `.route(` calls in `lib.rs`
//! itself and asserts every one is classified in a table above.
//!
//! Note on POST bodies: axum runs extractors in declaration order and every
//! protected handler declares `AuthedAccount` *before* its `Json` body, so an
//! unauthenticated POST is rejected before the body is parsed. The tables can
//! therefore send an empty body and still exercise the auth path.

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::{body_json, Harness};

/// A route under audit: HTTP method + path (with any `:param` already
/// substituted for a syntactically valid value).
struct Route {
    method: &'static str,
    path: &'static str,
    /// Why this route sits on the side of the boundary it does. Surfaces in
    /// assertion messages so a failure names the contract it broke.
    why: &'static str,
}

/// Routes that must reject an unauthenticated caller. Every one of these takes
/// `AuthedAccount` today; the test proves it still does.
const PROTECTED: &[Route] = &[
    Route {
        method: "POST",
        path: "/v1/attestations",
        why: "ingest writes to a specific account's ledger",
    },
    Route {
        method: "GET",
        path: "/v1/attestations",
        why: "search returns the caller's attestations",
    },
    Route {
        method: "GET",
        path: "/v1/attestations/11111111-1111-4111-8111-111111111111",
        why: "retrieval returns attestation payloads",
    },
    Route {
        method: "POST",
        path: "/v1/share-links",
        why: "minting a share link grants public read on private data",
    },
    Route {
        method: "DELETE",
        path: "/v1/share-links/sometoken",
        why: "revocation mutates the owning account's links",
    },
    Route {
        method: "GET",
        path: "/v1/export",
        why: "export streams the account's full corpus",
    },
    Route {
        method: "POST",
        path: "/v1/billing/portal",
        why: "the portal session is bound to the caller's Stripe customer",
    },
    Route {
        method: "GET",
        path: "/v1/account",
        why: "account summary is per-tenant data",
    },
    Route {
        method: "GET",
        path: "/v1/account/usage",
        why: "usage counters are per-tenant data",
    },
    Route {
        method: "GET",
        path: "/v1/account/api-keys",
        why: "listing keys must never be anonymous",
    },
    Route {
        method: "POST",
        path: "/v1/account/api-keys",
        why: "minting a key must never be anonymous",
    },
    Route {
        method: "DELETE",
        path: "/v1/account/api-keys/11111111-1111-4111-8111-111111111111",
        why: "revoking a key must never be anonymous",
    },
];

/// Routes that are public **by design**. Asserting on these is as load-bearing
/// as asserting on [`PROTECTED`]: it stops a future change from locking a
/// route the product depends on being open (share redemption, signup,
/// Stripe's webhook callback).
///
/// Admin routes (`/v1/admin/*`) are deliberately absent from both tables —
/// they are guarded by a manual `x-admin-key` check rather than
/// `AuthedAccount`, so they answer neither `401` nor `2xx` here. They get
/// their own assertion in [`admin_routes_reject_without_admin_key`].
const PUBLIC: &[Route] = &[
    Route {
        method: "GET",
        path: "/healthz",
        why: "platform health probe, hit before any key exists",
    },
    Route {
        method: "GET",
        path: "/",
        why: "root redirect to the dashboard",
    },
    Route {
        method: "GET",
        path: "/dashboard",
        why: "static shell; data is fetched client-side with a key",
    },
    Route {
        method: "GET",
        path: "/quickstart",
        why: "pre-signup marketing/onboarding page",
    },
    Route {
        method: "GET",
        path: "/v1/share-links/sometoken",
        why: "public redemption — the whole point of a share link",
    },
    Route {
        method: "GET",
        path: "/share/sometoken",
        why: "public redemption (HTML) — auditors hold no key",
    },
    Route {
        method: "POST",
        path: "/v1/account/signup",
        why: "signup is how a caller obtains their first key",
    },
    Route {
        method: "POST",
        path: "/v1/telemetry/usage",
        why: "privacy contract: SDK telemetry carries no account identity",
    },
    Route {
        method: "POST",
        path: "/v1/billing/checkout",
        why: "checkout runs pre-signup",
    },
    Route {
        method: "GET",
        path: "/billing/checkout",
        why: "landing-page checkout entry point, pre-signup",
    },
];

/// `POST /v1/billing/webhook` is public with respect to `x-api-key` but is
/// **not** unauthenticated: it is gated on a Stripe signature, and a caller
/// with no `stripe-signature` header gets a 401 that reuses
/// [`attestly_cloud::error::ApiError::Unauthorized`]. It therefore can't live
/// in [`PUBLIC`] (whose rule is "never answers 401"), and it must not live in
/// [`PROTECTED`] either — an API key does nothing for it. It gets its own
/// assertion in [`stripe_webhook_is_gated_on_signature_not_api_key`].
const SIGNATURE_GATED_ROUTES: usize = 1;

fn request(method: &str, path: &str, api_key: Option<&str>) -> Request<Body> {
    let mut b = Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json");
    if let Some(k) = api_key {
        b = b.header("x-api-key", k);
    }
    // `{}` parses as JSON for handlers that get far enough to read a body;
    // protected handlers reject on `AuthedAccount` before they ever do.
    b.body(Body::from("{}")).unwrap()
}

/// The contract from `API.md`: `unauthorized` — 401, missing or revoked API
/// key. Asserted rather than assumed, per the checklist.
async fn assert_unauthorized(h: &Harness, r: &Route, api_key: Option<&str>, case: &str) {
    let resp = h.send(request(r.method, r.path, api_key)).await;
    let (status, body) = body_json(resp).await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "{} {} ({case}) must be 401 — {}",
        r.method,
        r.path,
        r.why
    );
    assert_eq!(
        body["error"], "unauthorized",
        "{} {} ({case}) must use the `unauthorized` slug from API.md",
        r.method, r.path
    );
}

#[tokio::test]
async fn protected_routes_reject_missing_api_key() {
    let h = Harness::new().await;
    for r in PROTECTED {
        assert_unauthorized(&h, r, None, "no key").await;
    }
}

#[tokio::test]
async fn protected_routes_reject_wrong_api_key() {
    let h = Harness::new().await;
    // A syntactically valid UUID that was never issued — proves the check is a
    // real hash comparison, not a mere presence check on the header.
    for r in PROTECTED {
        assert_unauthorized(
            &h,
            r,
            Some("00000000-0000-4000-8000-000000000000"),
            "wrong key",
        )
        .await;
    }
}

#[tokio::test]
async fn protected_routes_admit_a_valid_api_key() {
    // The mirror of the two tests above: proves they fail for the *right*
    // reason. Without this, deleting `AuthedAccount` from a handler and
    // replacing it with an unconditional 401 would still pass the audit.
    let h = Harness::new().await;
    for r in PROTECTED {
        let resp = h.send(request(r.method, r.path, Some(&h.api_key))).await;
        let status = resp.status();
        assert_ne!(
            status,
            StatusCode::UNAUTHORIZED,
            "{} {} rejected a valid key — {}",
            r.method,
            r.path,
            r.why
        );
    }
}

#[tokio::test]
async fn public_routes_are_reachable_without_a_key() {
    let h = Harness::new().await;
    for r in PUBLIC {
        let resp = h.send(request(r.method, r.path, None)).await;
        let status = resp.status();
        // These may legitimately 400/404/422 on a stub body or bogus token —
        // what they must never do is demand a key.
        assert_ne!(
            status,
            StatusCode::UNAUTHORIZED,
            "{} {} must stay public — {}",
            r.method,
            r.path,
            r.why
        );
    }
}

#[tokio::test]
async fn admin_routes_reject_without_admin_key() {
    // Admin routes use a manual `x-admin-key` comparison instead of
    // `AuthedAccount`, so they're outside the tables above — but "not in the
    // table" must not mean "unguarded". An account API key must not open them
    // either.
    let h = Harness::new().await;
    let admin: &[(&str, &str)] = &[
        ("POST", "/v1/admin/bill-period"),
        ("GET", "/v1/admin/telemetry/conversion-ready"),
    ];
    for (method, path) in admin {
        for key in [None, Some(h.api_key.as_str())] {
            let resp = h.send(request(method, path, key)).await;
            let status = resp.status();
            assert!(
                status.is_client_error(),
                "{method} {path} must refuse a caller without the admin key (got {status})"
            );
            assert_ne!(
                status,
                StatusCode::OK,
                "{method} {path} must refuse a caller without the admin key"
            );
        }
    }
}

#[tokio::test]
async fn stripe_webhook_is_gated_on_signature_not_api_key() {
    // Two halves of one contract. First: no `stripe-signature` is refused,
    // so the endpoint isn't an open write path into billing state.
    let h = Harness::new().await;
    let resp = h.send(request("POST", "/v1/billing/webhook", None)).await;
    let (status, _) = body_json(resp).await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "webhook without a stripe-signature must be refused"
    );

    // Second: a valid *account* API key does not substitute for the
    // signature. Stripe never sends one, so accepting it would be a bypass.
    let resp = h
        .send(request("POST", "/v1/billing/webhook", Some(&h.api_key)))
        .await;
    let (status, _) = body_json(resp).await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "an API key must not stand in for a Stripe signature"
    );
}

#[tokio::test]
async fn route_tables_cover_every_route() {
    // The audit is only as good as its coverage: a new route in `lib.rs` that
    // nobody added a row for would otherwise sail through unexamined.
    //
    // The router count has to come from `lib.rs` itself. Comparing the table
    // sizes against a hand-written total would be a tautology over constants
    // in *this* file — it would still pass with a brand-new unclassified route
    // in the router, which is precisely the case it exists to catch. axum 0.7
    // exposes no route enumeration (`Router::has_routes` is a bool), so we
    // count the `.route(` calls in the source instead.
    //
    // 2 = the admin routes, covered by `admin_routes_reject_without_admin_key`.
    const ADMIN_ROUTES: usize = 2;

    let src = include_str!("../src/lib.rs");
    let in_router = src
        .split_once("pub fn router_with_rate_limit")
        .expect("router_with_rate_limit must exist in lib.rs")
        .1;
    let router_routes = in_router.matches(".route(").count();

    let classified = PROTECTED.len() + PUBLIC.len() + ADMIN_ROUTES + SIGNATURE_GATED_ROUTES;
    assert_eq!(
        classified, router_routes,
        "lib.rs wires {router_routes} routes but the auth audit classifies \
         {classified} — a route was added or removed without updating these \
         tables. Classify it as protected, public, admin, or signature-gated."
    );
}
