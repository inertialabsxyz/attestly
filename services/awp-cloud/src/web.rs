//! Server-side HTML helpers for the hosted viewer.
//!
//! The hosted viewer is intentionally minimal — its job is to bootstrap the
//! exact same in-browser verification path as the OSS audit viewer at
//! `tools/audit-viewer/`. The viewer's vendored `ed25519.js` and
//! `signing-payload.js` are served from `services/awp-cloud/web/` so
//! verification runs in the user's browser, independently of the server.
//!
//! This matters: if our server is compromised and starts returning bytes
//! that don't match the embedded signature, the page's JS verification will
//! visibly fail. That's the "tamper-evident even against a compromised
//! server" property called out in the API contract.

use awp_core::Attestation;
use chrono::{DateTime, Utc};

/// Render the share-link viewer page. Embeds the attestation JSON inline; the
/// page's JS verifies each attestation against its embedded `agent_pubkey`
/// before rendering it green.
pub fn share_html(token: &str, expires_at: &DateTime<Utc>, atts: &[Attestation]) -> String {
    let atts_json = serde_json::to_string(atts).unwrap_or_else(|_| "[]".to_string());
    let expires_str = expires_at.to_rfc3339();
    let count = atts.len();
    format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>AWP Cloud — Shared receipts</title>
<link rel="stylesheet" href="/static/viewer.css">
</head>
<body>
<header>
  <h1>AWP shared receipts</h1>
  <p class="lede">Token <code>{token}</code> · expires {expires_str} · {count} attestation(s)</p>
  <p class="lede">Each row re-verifies in your browser using a vendored
  ed25519 implementation. A green check means the server's bytes match the
  embedded signature; a red cross means something has been tampered with.</p>
</header>
<main>
  <section>
    <h2>Receipts</h2>
    <div id="receipts"></div>
  </section>
</main>
<script src="/static/ed25519.js"></script>
<script src="/static/signing-payload.js"></script>
<script>
const ATTS = {atts_json};
const signingPayloadBytes = signingPayload.signingPayloadBytes;

async function verifyAttestation(att) {{
    try {{
        const sig = att.signature;
        const payload = signingPayloadBytes(att);
        return await ed25519.verify(sig, payload, att.agent_pubkey);
    }} catch (_) {{
        return false;
    }}
}}

(async () => {{
    const container = document.getElementById('receipts');
    for (const att of ATTS) {{
        const ok = await verifyAttestation(att);
        const div = document.createElement('div');
        div.className = 'receipt';
        div.innerHTML = `
          <div class="verify-line ${{ok ? 'ok' : 'bad'}}">
            ${{ok ? '✓ verified in browser' : '✗ verification failed — possible tamper'}}
          </div>
          <div><strong>id</strong> <code>${{att.id}}</code></div>
          <div><strong>agent_id</strong> <code>${{att.agent_id}}</code></div>
          <div><strong>output</strong> <pre>${{att.output.replace(/[<>]/g, c => c === '<' ? '&lt;' : '&gt;')}}</pre></div>
          <hr>
        `;
        container.appendChild(div);
    }}
}})();
</script>
</body>
</html>"#
    )
}

/// 404 page for missing / expired / revoked share links.
pub fn not_found_html() -> String {
    r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>AWP Cloud — share link not found</title>
<link rel="stylesheet" href="/static/viewer.css">
</head>
<body>
<header>
  <h1>Share link not found</h1>
  <p class="lede">This share link has expired, been revoked, or never existed.</p>
</header>
</body>
</html>"#
        .to_string()
}

/// Banner page for share links whose underlying attestations failed
/// re-verification. We render this in place of the receipts so an auditor
/// sees the tamper signal directly rather than an opaque HTTP error.
pub fn tampered_html(detail: &str) -> String {
    format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>AWP Cloud — verification failed</title>
<link rel="stylesheet" href="/static/viewer.css">
</head>
<body>
<header>
  <h1 style="color: var(--red, #a02020)">Verification failed</h1>
  <p class="lede"><strong>One or more attestations at this share link failed re-verification.</strong></p>
  <p class="lede">Detail: <code>{detail}</code></p>
  <p class="lede">This page deliberately refuses to render the underlying
  receipts when re-verification fails. Contact the account owner.</p>
</header>
</body>
</html>"#
    )
}
