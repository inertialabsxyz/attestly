//! Server-side HTML helpers for the hosted viewer.
//!
//! The hosted viewer is intentionally minimal — its job is to bootstrap the
//! exact same in-browser verification path as the OSS audit viewer at
//! `tools/audit-viewer/`. The viewer's vendored `ed25519.js` and
//! `signing-payload.js` are served from `services/attestly-cloud/web/` so
//! verification runs in the user's browser, independently of the server.
//!
//! This matters: if our server is compromised and starts returning bytes
//! that don't match the embedded signature, the page's JS verification will
//! visibly fail. That's the "tamper-evident even against a compromised
//! server" property called out in the API contract.

use attestly_core::Attestation;
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
<title>Attestly Cloud — Shared receipts</title>
<link rel="stylesheet" href="/static/viewer.css">
</head>
<body>
<header>
  <h1>Attestly shared receipts</h1>
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
<title>Attestly Cloud — share link not found</title>
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
<title>Attestly Cloud — Dashboard</title>
<link rel="stylesheet" href="/static/viewer.css">
<style>
  /* The dashboard reuses viewer.css's light "paper" palette — no separate
     dark theme. Everything below derives from the shared :root variables. */
  .panel {
    background: var(--paper-card);
    border: 1px solid var(--rule);
    padding: 18px;
    margin: 18px 0;
  }
  .row { display: flex; gap: 16px; align-items: baseline; margin: 6px 0; }
  .row .label { color: var(--ink-faint); min-width: 180px; }
  .row .val { font-family: ui-monospace, "SF Mono", Menlo, monospace; }
  .keys table { width: 100%; border-collapse: collapse; }
  .keys th, .keys td {
    text-align: left; padding: 8px 6px;
    border-bottom: 1px solid var(--rule); font-size: 14px;
  }
  .btn {
    background: var(--ink); color: var(--paper-card);
    padding: 8px 14px; border: 1px solid var(--ink);
    cursor: pointer; font-weight: 600; font-size: 13px;
  }
  .btn:disabled { opacity: 0.5; cursor: default; }
  .btn.secondary { background: transparent; color: var(--ink); }
  .danger { color: var(--red); }
  .chart { display: flex; gap: 2px; align-items: flex-end; height: 80px; margin-top: 8px; }
  .chart .bar { background: var(--ink); width: 12px; }
  .field {
    padding: 8px; background: var(--paper-card); color: var(--ink);
    border: 1px solid var(--rule); font-size: 14px;
  }
  .notice {
    background: var(--paper-card); border: 1px solid var(--rule);
    border-left: 3px solid var(--ink); padding: 14px 18px; margin: 18px 0;
  }
  .notice code {
    display: block; padding: 10px; margin-top: 6px;
    background: #f4f1ea; word-break: break-all;
  }
  .msg { font-size: 13px; min-height: 18px; margin-top: 8px; }
  .msg.error { color: var(--red); }
  .msg.ok { color: var(--green); }
</style>
</head>
<body>
<header>
  <h1>Attestly Cloud — Dashboard</h1>
  <p class="lede">Plan, usage, and API keys for your account.</p>
</header>

<main>
  <div id="welcome" class="notice" style="display:none">
    <strong>Account ready.</strong>
    Your API key is shown once on first sign-in. Paste it below to load the dashboard,
    then store it in your secrets manager. We cannot recover it later.
    <div class="row" style="margin-top:10px">
      <input id="key-input" type="text" placeholder="x-api-key" class="field" style="flex:1"/>
      <button class="btn" id="load-btn" onclick="saveKey()">Load dashboard</button>
    </div>
    <div id="welcome-msg" class="msg"></div>
  </div>

  <div id="dashboard" style="display:none">
  <section class="panel" id="plan-panel">
    <h2>Plan</h2>
    <div id="plan-rows"></div>
    <div style="margin-top:14px">
      <button class="btn" onclick="openPortal()">Manage subscription</button>
      <button class="btn secondary" onclick="downloadExport()">Export everything (JSONL)</button>
    </div>
    <div id="plan-msg" class="msg"></div>
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
      <input id="new-key-name" placeholder="project-name" class="field"/>
      <button class="btn" onclick="createKey()">Create key</button>
    </div>
    <div id="keys-msg" class="msg"></div>
    <div id="new-key-banner" class="notice" style="display:none; margin-top:14px"></div>
  </section>
  </div>
</main>

<script>
const KEY_STORAGE = 'attestly-cloud-api-key';
function getKey() { return localStorage.getItem(KEY_STORAGE) || ''; }
function esc(s) { return String(s).replace(/[<>&]/g, c => ({'<':'&lt;','>':'&gt;','&':'&amp;'}[c])); }
function showMsg(id, text, kind) {
  const el = document.getElementById(id);
  el.textContent = text || '';
  el.className = 'msg' + (text ? ' ' + (kind || 'error') : '');
}

/// Error thrown by `api()` when the server returns a non-2xx status, carrying
/// the server's `detail` so callers can surface a real message.
class ApiError extends Error {
  constructor(status, detail) { super(detail); this.status = status; }
}
async function api(method, path, body) {
  const headers = { 'x-api-key': getKey() };
  if (body) headers['content-type'] = 'application/json';
  const r = await fetch(path, { method, headers, body: body ? JSON.stringify(body) : undefined });
  if (r.status === 204) return null;
  let payload = null;
  try { payload = await r.json(); } catch (_) { /* non-JSON body */ }
  if (!r.ok) {
    const detail = (payload && (payload.detail || payload.error)) || ('HTTP ' + r.status);
    throw new ApiError(r.status, detail);
  }
  return payload;
}

async function saveKey() {
  const v = document.getElementById('key-input').value.trim();
  if (!v) { showMsg('welcome-msg', 'Paste your x-api-key first.'); return; }
  const btn = document.getElementById('load-btn');
  btn.disabled = true;
  showMsg('welcome-msg', '');
  // Validate the key against /v1/account before committing it to storage —
  // otherwise a bad key gets saved and every panel fails silently.
  localStorage.setItem(KEY_STORAGE, v);
  try {
    await api('GET', '/v1/account');
  } catch (e) {
    localStorage.removeItem(KEY_STORAGE);
    btn.disabled = false;
    showMsg('welcome-msg', e.status === 401
      ? 'That key was not accepted. Check it and try again.'
      : ('Could not load the dashboard: ' + e.message));
    return;
  }
  document.getElementById('welcome').style.display = 'none';
  document.getElementById('dashboard').style.display = 'block';
  loadAll();
}

async function loadAll() {
  if (!getKey()) { showWelcome(); return; }
  document.getElementById('dashboard').style.display = 'block';
  await Promise.all([loadPlan(), loadUsage(), loadKeys()]);
}
function showWelcome() {
  document.getElementById('welcome').style.display = 'block';
  document.getElementById('dashboard').style.display = 'none';
}

async function loadPlan() {
  try {
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
      .map(([k, v]) => `<div class="row"><span class="label">${esc(k)}</span><span class="val">${esc(v)}</span></div>`).join('');
  } catch (e) {
    showMsg('plan-msg', 'Could not load plan: ' + e.message);
  }
}
async function loadUsage() {
  try {
    const u = await api('GET', '/v1/account/usage');
    const total = u.points.reduce((s, p) => s + p.count, 0);
    document.getElementById('usage-total').textContent = total.toLocaleString() + ' attestations';
    const max = Math.max(1, ...u.points.map(p => p.count));
    const chart = document.getElementById('usage-chart');
    chart.innerHTML = u.points.map(p => {
      const h = Math.max(2, Math.round((p.count / max) * 78));
      return `<div class="bar" title="${esc(p.day)}: ${esc(p.count)}" style="height:${h}px"></div>`;
    }).join('');
  } catch (e) {
    document.getElementById('usage-total').textContent = 'Could not load usage: ' + e.message;
  }
}
async function loadKeys() {
  try {
    const keys = await api('GET', '/v1/account/api-keys');
    const tbody = document.getElementById('keys-body');
    tbody.innerHTML = keys.map(k => `
      <tr>
        <td>${esc(k.project_name)}</td>
        <td>${new Date(k.created_at).toLocaleDateString()}</td>
        <td><code>${esc(k.id)}</code></td>
        <td><button class="btn secondary" onclick="revokeKey('${esc(k.id)}')">Revoke</button></td>
      </tr>
    `).join('');
  } catch (e) {
    showMsg('keys-msg', 'Could not load API keys: ' + e.message);
  }
}
async function createKey() {
  const name = document.getElementById('new-key-name').value.trim();
  if (!name) { showMsg('keys-msg', 'Enter a project name first.'); return; }
  showMsg('keys-msg', '');
  let r;
  try {
    r = await api('POST', '/v1/account/api-keys', { project_name: name });
  } catch (e) {
    showMsg('keys-msg', 'Could not create key: ' + e.message);
    return;
  }
  const banner = document.getElementById('new-key-banner');
  banner.style.display = 'block';
  banner.innerHTML = `<strong>New API key</strong> for project <code style="display:inline; padding:1px 4px">${esc(r.project_name)}</code><br>
    Copy this now — it will not be shown again:<br>
    <code>${esc(r.key)}</code>`;
  document.getElementById('new-key-name').value = '';
  await loadKeys();
}
async function revokeKey(id) {
  if (!confirm('Revoke this key? Existing requests with it will fail.')) return;
  try {
    await api('DELETE', '/v1/account/api-keys/' + id);
  } catch (e) {
    showMsg('keys-msg', 'Could not revoke key: ' + e.message);
    return;
  }
  await loadKeys();
}
async function openPortal() {
  try {
    const r = await api('POST', '/v1/billing/portal');
    if (r && r.url) {
      window.location = r.url;
    } else {
      showMsg('plan-msg', 'Billing portal is unavailable for this account.');
    }
  } catch (e) {
    showMsg('plan-msg', 'Could not open the billing portal: ' + e.message);
  }
}
async function downloadExport() {
  showMsg('plan-msg', 'Preparing export…', 'ok');
  try {
    const r = await fetch('/v1/export', { headers: { 'x-api-key': getKey() } });
    if (!r.ok) {
      let detail = 'HTTP ' + r.status;
      try { const b = await r.json(); detail = b.detail || b.error || detail; } catch (_) {}
      showMsg('plan-msg', 'Export failed: ' + detail);
      return;
    }
    const b = await r.blob();
    const url = URL.createObjectURL(b);
    const a = document.createElement('a');
    a.href = url;
    a.download = 'attestly-attestations.jsonl';
    document.body.appendChild(a);
    a.click();
    a.remove();
    // Revoke after a tick — revoking synchronously after click() can cancel
    // the download in some browsers before it has started.
    setTimeout(() => URL.revokeObjectURL(url), 4000);
    showMsg('plan-msg', 'Export downloaded.', 'ok');
  } catch (e) {
    showMsg('plan-msg', 'Export failed: ' + e.message);
  }
}

const params = new URLSearchParams(window.location.search);
if (params.get('welcome') === '1' && !getKey()) {
  showWelcome();
} else if (!getKey()) {
  showWelcome();
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
///   2. Display `pip install attestly-langgraph` and the freshly-minted API key
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
<title>Attestly Cloud — Quickstart</title>
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
      <pre id="pip-snippet">pip install attestly-langgraph</pre>
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
from attestly.langgraph import attest, CloudSink
from langgraph.graph import StateGraph

graph = build_my_graph()
graph = attest(graph, agent_id="quickstart-agent",
               sink=CloudSink(api_key=os.environ["ATTESTLY_API_KEY"]))
graph.invoke({{"hello": "world"}})</pre>
      <button class="btn secondary" onclick="copyText('snippet', this)">Copy</button>
    </div>
    <p style="color:#8a8680; font-size:13px; margin:12px 0 0 0;">
      Set <code>ATTESTLY_API_KEY</code> in your environment to the key above before
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
const KEY_STORAGE = 'attestly-cloud-api-key';

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
<title>Attestly Cloud — verification failed</title>
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
