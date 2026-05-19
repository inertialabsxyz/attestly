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

/// Account dashboard shell. The page reads the user's `x-api-key` from
/// `localStorage` (the post-signup welcome banner writes it there) and
/// then pulls plan, usage, and key data from `/v1/account/*`.
pub fn dashboard_html() -> String {
    r##"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>AWP Cloud — Dashboard</title>
<link rel="stylesheet" href="/static/viewer.css">
<style>
  .panel { border: 1px solid #2a2a2a; border-radius: 8px; padding: 18px; margin: 18px 0; }
  .row { display: flex; gap: 16px; align-items: baseline; margin: 6px 0; }
  .row .label { color: #8a8680; min-width: 180px; }
  .row .val { font-family: ui-monospace, "SF Mono", Menlo, monospace; }
  .keys table { width: 100%; border-collapse: collapse; }
  .keys th, .keys td { text-align: left; padding: 8px 6px; border-bottom: 1px solid #2a2a2a; font-size: 14px; }
  .btn { background: #d9ff3d; color: #0a0a0a; padding: 8px 14px; border-radius: 999px; border: none; cursor: pointer; font-weight: 600; }
  .btn.secondary { background: transparent; color: inherit; border: 1px solid #2a2a2a; }
  .danger { color: #ff6b6b; }
  .chart { display: flex; gap: 2px; align-items: flex-end; height: 80px; margin-top: 8px; }
  .chart .bar { background: #d9ff3d; width: 12px; }
  .welcome { background: #1c2410; border: 1px solid #d9ff3d; padding: 14px 18px; border-radius: 8px; margin: 18px 0; }
</style>
</head>
<body>
<header>
  <h1>AWP Cloud — Dashboard</h1>
  <p class="lede">Plan, usage, and API keys for your account.</p>
</header>

<main>
  <div id="welcome" class="welcome" style="display:none">
    <strong>Account ready.</strong>
    Your API key is shown once on first sign-in. Paste it below to load the dashboard,
    then store it in your secrets manager. We cannot recover it later.
    <div class="row" style="margin-top:10px">
      <input id="key-input" type="text" placeholder="x-api-key" style="flex:1; padding:8px; background:#0a0a0a; color:#f5f3ef; border:1px solid #2a2a2a; border-radius:4px"/>
      <button class="btn" onclick="saveKey()">Load dashboard</button>
    </div>
  </div>

  <section class="panel" id="plan-panel">
    <h2>Plan</h2>
    <div id="plan-rows"></div>
    <div style="margin-top:14px">
      <button class="btn" onclick="openPortal()">Manage subscription</button>
      <a class="btn secondary" href="#" onclick="downloadExport(); return false">Export everything (JSONL)</a>
    </div>
  </section>

  <section class="panel" id="usage-panel">
    <h2>Usage — last 30 days</h2>
    <div id="usage-total"></div>
    <div id="usage-chart" class="chart"></div>
  </section>

  <section class="panel keys" id="keys-panel">
    <h2>API keys</h2>
    <table id="keys-table">
      <thead><tr><th>Project</th><th>Created</th><th>Id</th><th></th></tr></thead>
      <tbody id="keys-body"></tbody>
    </table>
    <div style="margin-top:14px">
      <input id="new-key-name" placeholder="project-name" style="padding:8px; background:#0a0a0a; color:#f5f3ef; border:1px solid #2a2a2a; border-radius:4px"/>
      <button class="btn" onclick="createKey()">Create key</button>
    </div>
    <div id="new-key-banner" class="welcome" style="display:none; margin-top:14px"></div>
  </section>
</main>

<script>
const KEY_STORAGE = 'awp-cloud-api-key';
function getKey() { return localStorage.getItem(KEY_STORAGE) || ''; }
function saveKey() {
  const v = document.getElementById('key-input').value.trim();
  if (!v) return;
  localStorage.setItem(KEY_STORAGE, v);
  document.getElementById('welcome').style.display = 'none';
  loadAll();
}
async function api(method, path, body) {
  const headers = { 'x-api-key': getKey() };
  if (body) headers['content-type'] = 'application/json';
  const r = await fetch(path, { method, headers, body: body ? JSON.stringify(body) : undefined });
  if (r.status === 204) return null;
  return r.json();
}
async function loadAll() {
  if (!getKey()) { document.getElementById('welcome').style.display = 'block'; return; }
  await Promise.all([loadPlan(), loadUsage(), loadKeys()]);
}
async function loadPlan() {
  const a = await api('GET', '/v1/account');
  const rows = [
    ['Email', a.email],
    ['Plan', a.plan],
    ['Retention', a.retention_days + ' days'],
    ['Included', a.included_attestations.toLocaleString() + ' attestations / period'],
    ['Used this period', (a.usage_this_period || 0).toLocaleString()],
    ['Stripe customer', a.stripe_customer_id || '(none)'],
  ];
  document.getElementById('plan-rows').innerHTML = rows
    .map(([k, v]) => `<div class="row"><span class="label">${k}</span><span class="val">${v}</span></div>`).join('');
}
async function loadUsage() {
  const u = await api('GET', '/v1/account/usage');
  const total = u.points.reduce((s, p) => s + p.count, 0);
  document.getElementById('usage-total').textContent = total.toLocaleString() + ' attestations';
  const max = Math.max(1, ...u.points.map(p => p.count));
  const chart = document.getElementById('usage-chart');
  chart.innerHTML = u.points.map(p => {
    const h = Math.max(2, Math.round((p.count / max) * 78));
    return `<div class="bar" title="${p.day}: ${p.count}" style="height:${h}px"></div>`;
  }).join('');
}
async function loadKeys() {
  const keys = await api('GET', '/v1/account/api-keys');
  const tbody = document.getElementById('keys-body');
  tbody.innerHTML = keys.map(k => `
    <tr>
      <td>${k.project_name}</td>
      <td>${new Date(k.created_at).toLocaleDateString()}</td>
      <td><code>${k.id}</code></td>
      <td><button class="btn secondary" onclick="revokeKey('${k.id}')">Revoke</button></td>
    </tr>
  `).join('');
}
async function createKey() {
  const name = document.getElementById('new-key-name').value.trim();
  if (!name) return;
  const r = await api('POST', '/v1/account/api-keys', { project_name: name });
  const banner = document.getElementById('new-key-banner');
  banner.style.display = 'block';
  banner.innerHTML = `<strong>New API key</strong> for project <code>${r.project_name}</code><br>
    Copy this now — it will not be shown again:<br>
    <code style="display:block; padding:10px; margin-top:6px; background:#0a0a0a; border-radius:4px">${r.key}</code>`;
  await loadKeys();
}
async function revokeKey(id) {
  if (!confirm('Revoke this key? Existing requests with it will fail.')) return;
  await fetch('/v1/account/api-keys/' + id, { method: 'DELETE', headers: { 'x-api-key': getKey() } });
  await loadKeys();
}
async function openPortal() {
  const r = await api('POST', '/v1/billing/portal');
  if (r && r.url) window.location = r.url;
}
function downloadExport() {
  const url = '/v1/export';
  fetch(url, { headers: { 'x-api-key': getKey() } })
    .then(r => r.blob())
    .then(b => {
      const a = document.createElement('a');
      a.href = URL.createObjectURL(b);
      a.download = 'awp-attestations.jsonl';
      a.click();
    });
}
const params = new URLSearchParams(window.location.search);
if (params.get('welcome') === '1' && !getKey()) {
  document.getElementById('welcome').style.display = 'block';
} else {
  loadAll();
}
</script>
</body>
</html>"##.to_string()
}

/// Server-rendered quickstart page. Walks a new user through:
///
///   1. Sign up with email + password (POSTs `/v1/account/signup`)
///   2. Display `pip install awp-langgraph` and the freshly-minted API key
///   3. Copy a 5-line snippet wrapping a tiny LangGraph example
///   4. Live-poll `/v1/account/usage` until the first attestation lands,
///      then redirect to the dashboard
///
/// `base_url` is the public origin used to compose absolute dashboard links
/// in the final-step banner; on a local dev box it's `http://localhost:8080`.
pub fn quickstart_html(base_url: &str) -> String {
    let safe_base = base_url.replace('"', "&quot;");
    format!(
        r##"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>AWP Cloud — Quickstart</title>
<link rel="stylesheet" href="/static/viewer.css">
<style>
  body {{ max-width: 880px; margin: 0 auto; padding: 32px; }}
  .step {{ border: 1px solid #2a2a2a; border-radius: 8px; padding: 24px; margin: 18px 0;
           opacity: 0.55; transition: opacity 180ms ease, border-color 180ms ease; }}
  .step.active {{ opacity: 1; border-color: #d9ff3d; }}
  .step.done   {{ opacity: 1; border-color: #2f7a3a; }}
  .step h2 {{ margin: 0 0 12px 0; font-size: 18px; letter-spacing: -0.005em; }}
  .step h2 .num {{ color: #d9ff3d; margin-right: 10px; }}
  .step.done h2 .num {{ color: #2f7a3a; }}
  .step.done h2 .num::after {{ content: " ✓"; }}
  .form-row {{ display: flex; gap: 12px; align-items: center; flex-wrap: wrap; }}
  .form-row input {{ flex: 1 0 220px; padding: 10px 12px; background: #0a0a0a; color: #f5f3ef;
                     border: 1px solid #2a2a2a; border-radius: 4px; font-size: 14px; }}
  pre, code.block {{ display: block; background: #0a0a0a; color: #f5f3ef; padding: 14px 16px;
                     border-radius: 4px; font-family: ui-monospace, "SF Mono", Menlo, monospace;
                     font-size: 13px; white-space: pre-wrap; word-break: break-all;
                     border: 1px solid #2a2a2a; }}
  .btn {{ background: #d9ff3d; color: #0a0a0a; padding: 10px 16px; border-radius: 999px;
          border: none; cursor: pointer; font-weight: 600; font-size: 14px; }}
  .btn.secondary {{ background: transparent; color: inherit; border: 1px solid #2a2a2a; }}
  .copy-row {{ display: flex; gap: 10px; align-items: stretch; }}
  .copy-row pre {{ flex: 1; margin: 0; }}
  .danger {{ color: #ff6b6b; min-height: 20px; }}
  .ok {{ color: #8aff8a; }}
  .pulse {{ display: inline-block; width: 10px; height: 10px; border-radius: 50%;
            background: #d9ff3d; margin-right: 8px;
            animation: pulse 1.4s ease-in-out infinite; }}
  @keyframes pulse {{ 0%, 100% {{ opacity: 0.35 }} 50% {{ opacity: 1 }} }}
</style>
</head>
<body>
<header>
  <h1>Quickstart — your first signed receipt in 60 seconds</h1>
  <p class="lede">Sign up, install the SDK, paste five lines, watch the first
  attestation land in your dashboard. No cloud-side magic — every receipt is
  signed locally in your process and POSTed here for retention and sharing.</p>
</header>

<main>

  <section id="step-signup" class="step active">
    <h2><span class="num">1</span>Create your account</h2>
    <div class="form-row">
      <input id="signup-email" type="email" autocomplete="email" placeholder="you@company.com">
      <input id="signup-password" type="password" autocomplete="new-password" placeholder="password (8+ chars)">
      <button class="btn" id="signup-btn" onclick="signup()">Sign up</button>
    </div>
    <p class="danger" id="signup-error" role="alert"></p>
    <p style="color:#8a8680; font-size:13px; margin:8px 0 0 0;">Free Team-tier trial — 1M attestations / month, no credit card.</p>
  </section>

  <section id="step-key" class="step">
    <h2><span class="num">2</span>Install the SDK and grab your API key</h2>
    <div class="copy-row">
      <pre id="pip-snippet">pip install awp-langgraph</pre>
      <button class="btn secondary" onclick="copyText('pip-snippet', this)">Copy</button>
    </div>
    <p style="color:#8a8680; font-size:13px; margin:14px 0 6px 0;">Your API key (shown once — paste it into your secrets manager):</p>
    <div class="copy-row">
      <pre id="api-key-display">—</pre>
      <button class="btn secondary" onclick="copyText('api-key-display', this)">Copy</button>
    </div>
  </section>

  <section id="step-snippet" class="step">
    <h2><span class="num">3</span>Paste five lines into your agent</h2>
    <div class="copy-row">
      <pre id="snippet">import os
from awp.langgraph import attest, CloudSink
from langgraph.graph import StateGraph

graph = build_my_graph()
graph = attest(graph, agent_id="quickstart-agent",
               sink=CloudSink(api_key=os.environ["AWP_API_KEY"]))
graph.invoke({{"hello": "world"}})</pre>
      <button class="btn secondary" onclick="copyText('snippet', this)">Copy</button>
    </div>
    <p style="color:#8a8680; font-size:13px; margin:12px 0 0 0;">
      Set <code>AWP_API_KEY</code> in your environment to the key above before
      running. No graph yet? <code>build_my_graph()</code> is whatever
      <code>StateGraph</code> you've already got — see the
      <a href="/docs/quickstart" style="color:#d9ff3d">docs</a> for a runnable
      KYC example.
    </p>
  </section>

  <section id="step-attestation" class="step">
    <h2><span class="num">4</span>Watch for your first attestation</h2>
    <p id="poll-status"><span class="pulse"></span>Waiting for the first signed receipt to land…</p>
    <p style="color:#8a8680; font-size:13px;">This page polls <code>/v1/account/usage</code>
    once every two seconds. Run the snippet above and the counter will tick.
    First attestation usually lands within a couple of seconds.</p>
    <p><a class="btn secondary" id="dashboard-link" href="{safe_base}/dashboard?welcome=1">Open dashboard</a></p>
  </section>

</main>

<script>
const BASE = {base_url_js};
const KEY_STORAGE = 'awp-cloud-api-key';

function setActive(id) {{
  document.querySelectorAll('.step').forEach(s => s.classList.remove('active'));
  document.getElementById(id).classList.add('active');
}}
function setDone(id) {{
  document.getElementById(id).classList.remove('active');
  document.getElementById(id).classList.add('done');
}}

async function signup() {{
  const email = document.getElementById('signup-email').value.trim();
  const password = document.getElementById('signup-password').value;
  const err = document.getElementById('signup-error');
  err.textContent = '';
  if (!email || !password) {{ err.textContent = 'Email and password are required.'; return; }}
  if (password.length < 8) {{ err.textContent = 'Password must be at least 8 characters.'; return; }}
  const btn = document.getElementById('signup-btn');
  btn.disabled = true; btn.textContent = 'Signing up…';
  try {{
    const r = await fetch('/v1/account/signup', {{
      method: 'POST',
      headers: {{ 'content-type': 'application/json' }},
      body: JSON.stringify({{ email, password }})
    }});
    if (!r.ok) {{
      const body = await r.json().catch(() => ({{detail: 'signup failed'}}));
      err.textContent = body.detail || 'signup failed';
      btn.disabled = false; btn.textContent = 'Sign up';
      return;
    }}
    const body = await r.json();
    localStorage.setItem(KEY_STORAGE, body.api_key);
    document.getElementById('api-key-display').textContent = body.api_key;
    document.getElementById('dashboard-link').href = body.dashboard_url;
    setDone('step-signup');
    setActive('step-key');
    setTimeout(() => setActive('step-snippet'), 600);
    startPolling();
  }} catch (e) {{
    err.textContent = 'Network error — please retry.';
    btn.disabled = false; btn.textContent = 'Sign up';
  }}
}}

function copyText(id, btn) {{
  const text = document.getElementById(id).textContent;
  navigator.clipboard.writeText(text).then(() => {{
    const original = btn.textContent;
    btn.textContent = 'Copied';
    setTimeout(() => {{ btn.textContent = original; }}, 1200);
  }});
}}

let pollTimer = null;
function startPolling() {{
  const key = localStorage.getItem(KEY_STORAGE);
  if (!key) return;
  let attempts = 0;
  if (pollTimer) clearInterval(pollTimer);
  pollTimer = setInterval(async () => {{
    attempts += 1;
    try {{
      const r = await fetch('/v1/account/usage', {{ headers: {{ 'x-api-key': key }} }});
      if (!r.ok) return;
      const body = await r.json();
      const total = (body.points || []).reduce((s, p) => s + p.count, 0);
      if (total > 0) {{
        clearInterval(pollTimer);
        setDone('step-snippet');
        setDone('step-attestation');
        document.getElementById('poll-status').innerHTML =
          '<span class="ok">✓ First attestation received.</span> '
          + total.toLocaleString() + ' receipt(s) so far.';
      }} else if (attempts > 120) {{
        // Stop after ~4 minutes to avoid an infinite poll.
        clearInterval(pollTimer);
        document.getElementById('poll-status').textContent =
          'No attestations yet. Run the snippet, then refresh the dashboard.';
      }}
    }} catch (_) {{ /* swallow — keep polling */ }}
  }}, 2000);
}}

// If the user already signed up earlier this session, resume mid-flow.
const existing = localStorage.getItem(KEY_STORAGE);
if (existing) {{
  document.getElementById('api-key-display').textContent = existing;
  setDone('step-signup');
  setActive('step-snippet');
  startPolling();
}}
</script>
</body>
</html>"##,
        safe_base = safe_base,
        base_url_js = serde_json::to_string(base_url).unwrap_or_else(|_| "\"\"".to_string()),
    )
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
