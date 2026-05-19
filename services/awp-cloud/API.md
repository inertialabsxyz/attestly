# `awp-cloud` HTTP API — locked contract (v1)

This document is the **load-bearing contract** between `awp-cloud` (this
service) and clients that emit attestations into it. The LangGraph SDK's
`CloudSink` (GTM Phase 2 Step 3) codes against this surface; Stripe billing
(Step 4) attaches to the same `/v1/` namespace.

Once this document is merged to `main`, the request and response shapes of the
ingest path **must not change**. Additive fields are permitted; removals or
renames require a new versioned namespace.

## Base URL

```
https://app.awp-cloud.xyz/v1/   (staging: https://awp-cloud-staging.fly.dev/v1/)
http://localhost:8080/v1/        (local dev via docker compose)
```

## Authentication

Every request to a `/v1/` route other than `POST /v1/share-links/{token}`
(public share link redemption) and `GET /healthz` requires an API key in the
`x-api-key` header:

```
x-api-key: <uuid-key-issued-by-the-account-dashboard>
```

Keys are 36-character UUIDs. They are issued per project under one account and
hashed (Argon2id) at rest. A revoked key returns `401 Unauthorized`. A missing
key returns `401`. A key for a different account that owns no record for the
target id returns `404 Not Found` (deliberately — we do not leak existence).

## Endpoints

### `POST /v1/attestations`

Ingest a single signed attestation. The server validates the embedded Ed25519
signature against `agent_pubkey` before accepting. Validation runs against the
canonical signing payload defined by `Attestation::signing_payload` in
`crates/awp-core/src/attestation.rs` — the contract is byte-identical to the
Rust core; clients that produce other encodings will fail validation.

**Request**

```http
POST /v1/attestations HTTP/1.1
Host: app.awp-cloud.xyz
x-api-key: <key>
content-type: application/json
```

Body: a JSON-encoded `Attestation`, matching the schema in
`crates/awp-core/src/attestation.rs`:

```json
{
  "id":           "550e8400-e29b-41d4-a716-446655440000",
  "agent_id":     "agent-kyc-01",
  "agent_pubkey": "<64 hex chars>",
  "task_hash":    "<64 hex chars>",
  "output_hash":  "<64 hex chars>",
  "output":       "<arbitrary string, usually JSON>",
  "status":       "Completed"
                  | { "Failed": "..." }
                  | { "Verified": { "attestation_valid": true,
                                    "answer_correct":   true } },
  "references":   "<uuid>" | null,
  "timestamp":    1700000000,
  "signature":    "<128 hex chars>"
}
```

**Response — success (`201 Created`)**

```json
{
  "id":          "550e8400-e29b-41d4-a716-446655440000",
  "received_at": "2026-05-19T14:33:21Z",
  "blob_sha256": "<64 hex chars>"
}
```

**Response — invalid signature (`422 Unprocessable Entity`)**

```json
{
  "error":  "signature_invalid",
  "detail": "ed25519 verification failed against embedded agent_pubkey"
}
```

**Response — malformed JSON (`400 Bad Request`)**

```json
{
  "error":  "invalid_request",
  "detail": "missing required field `agent_pubkey`"
}
```

**Response — duplicate (`200 OK`)** — idempotent: the same attestation id
posted twice returns the existing record's metadata. The body is identical to
the `201` shape, but the status code is `200` so clients can distinguish.

### `GET /v1/attestations`

Paginated search. All filter parameters are optional and combine with `AND`.
`customer_id` is matched against the `output` field after JSON-parsing it (KYC
attestations encode `{"customer_id": "..."}` in their output). Pagination is
cursor-based; `cursor` is opaque, returned as `next_cursor` in the previous
page.

**Query parameters**

| Name          | Type    | Default | Notes                                            |
|---------------|---------|---------|--------------------------------------------------|
| `agent_id`    | string  | none    | Exact match                                      |
| `customer_id` | string  | none    | Exact match against `output.customer_id`         |
| `from`        | string  | none    | ISO-8601 timestamp, inclusive                    |
| `to`          | string  | none    | ISO-8601 timestamp, exclusive                    |
| `cursor`      | string  | none    | Opaque cursor from a previous response           |
| `limit`       | integer | 50      | 1..=500. Exceeding 500 returns `400`.            |

**Response (`200 OK`)**

```json
{
  "attestations": [
    {
      "id":           "...",
      "agent_id":     "...",
      "agent_pubkey": "...",
      "customer_id":  "..." | null,
      "received_at":  "2026-05-19T14:33:21Z",
      "blob_sha256":  "...",
      "status":       "Completed" | { "Failed": "..." } | { "Verified": {...} }
    }
  ],
  "next_cursor": "..." | null
}
```

This response shape contains **metadata only**. To fetch the signed canonical
attestation bytes, follow up with `GET /v1/attestations/{id}`.

### `GET /v1/attestations/{id}`

Fetch a single attestation. Returns the full `Attestation` JSON, re-verified
server-side against its embedded `agent_pubkey` immediately before returning
(tamper detection — see below).

**Response — success (`200 OK`)**: full `Attestation` body, identical shape to
`POST /v1/attestations` request body.

**Response — tampered at rest (`422 Unprocessable Entity`)**:

```json
{
  "error":  "signature_invalid",
  "detail": "stored attestation failed re-verification — possible tamper at rest"
}
```

This is the load-bearing tamper guarantee. The server **never** returns an
attestation that does not currently re-verify against its own embedded pubkey,
even if it verified at ingest. Auditors can rely on the 200 response shape as
a fresh cryptographic statement, not a stale acceptance record.

**Response — not found (`404`)**: `{"error":"not_found"}`. Returned for both
"no such id" and "id belongs to another account" — see Authentication above.

### `POST /v1/share-links`

Create a tokenised, public, time-limited share URL for one or more
attestations.

**Request**

```json
{
  "attestation_ids": ["<uuid>", "<uuid>"],
  "expires_in_days": 30
}
```

or, equivalently, with a filter:

```json
{
  "filter": {
    "agent_id":    "agent-kyc-01",
    "customer_id": "4711",
    "from":        "2026-05-01T00:00:00Z",
    "to":          "2026-06-01T00:00:00Z"
  },
  "expires_in_days": 30
}
```

`expires_in_days` defaults to 30, max 365. Exceeding the max returns `400`.
Exactly one of `attestation_ids` and `filter` must be supplied.

**Response (`201 Created`)**

```json
{
  "token":      "<48-char url-safe base64>",
  "url":        "https://app.awp-cloud.xyz/share/<token>",
  "expires_at": "2026-06-18T14:33:21Z"
}
```

### `GET /share/{token}` (public)

Server-rendered viewer page. Requires no API key. Renders the share link's
attestation(s) using the same in-browser verification JS as the OSS audit
viewer, so the viewer re-verifies independently of the server. If the token is
expired or revoked the page renders an explanatory 404.

### `GET /v1/share-links/{token}` (public, JSON)

Machine-readable form of `GET /share/{token}`. Returns the underlying
attestation(s) as a JSON array, suitable for piping into the OSS static
viewer.

**Response (`200 OK`)**

```json
{
  "expires_at":   "2026-06-18T14:33:21Z",
  "attestations": [ { ...full Attestation... } ]
}
```

**Response — expired or revoked (`404`)**: `{"error":"link_not_found"}`.

### `DELETE /v1/share-links/{token}`

Revoke a share link. Requires the API key that owns the link's source
attestations.

**Response (`204 No Content`)** on success.

### `GET /v1/export`

Stream every attestation owned by the authenticated account as JSON Lines
(`application/x-ndjson`). One attestation per line, full canonical body
identical to `GET /v1/attestations/{id}`. Used by the "free to leave"
guarantee on the pricing page (Step 4) — a customer cancelling the Team tier
can run a single `curl` to retrieve every receipt they ever wrote.

The endpoint streams; expect a connection that stays open for the duration of
the export. There is no pagination.

### `POST /v1/billing/webhook` (Stripe; Step 4 fills in)

Stub during Step 2 — returns `501 Not Implemented`. Step 4 wires this to
Stripe's webhook signing. The route is pre-placed so the routing layer does
not change in Step 4.

### `GET /healthz` (public)

Returns `200 OK` with `{"status":"ok","version":"<git sha>"}` if the service
can reach Postgres and blob storage. Returns `503` otherwise. Used by
`flyctl` / Railway health probes.

## Error response shape (all routes)

Every non-2xx response uses the same envelope:

```json
{ "error": "<machine-readable slug>", "detail": "<human-readable message>" }
```

Slugs in use:

- `invalid_request` — 400, malformed JSON or violated parameter constraints
- `unauthorized` — 401, missing or revoked API key
- `not_found` — 404
- `signature_invalid` — 422, signature did not verify (ingest or retrieval)
- `link_not_found` — 404, share-link token expired or revoked
- `internal` — 500, server-side failure with no user-visible cause

## Canonical encoding

The signing payload (the bytes the signature covers) is produced by
`Attestation::signing_payload` in `crates/awp-core/src/attestation.rs`. Clients
that produce attestations in other languages **must** match that encoding
byte-for-byte. The JS implementation in
`tools/audit-viewer/signing-payload.js` and the Python implementation in
`crates/awp-python/` (Step 1) both reproduce this exactly; cross-language
byte-equality tests are mandatory for every new client.

## Versioning

This is `v1`. Additive fields on responses are non-breaking. Removals,
renames, or semantic changes require `v2`. Both versions are served in
parallel during a deprecation window of at least 90 days announced in
release notes.
