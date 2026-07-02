/* ========================================================================
   ZeroSense — Landing Page Interactions
   Tab switching · Scroll reveal · Animated counters (spade.com style)
   Kinetic tagline · FAQ accordion · Live proof feed
   ======================================================================== */

/* ─── Pipeline tab switching ──────────────────────────────── */
function switchTab(idx) {
  document.querySelectorAll('.pipeline-tab').forEach((t, i) => t.classList.toggle('active', i === idx));
  document.querySelectorAll('.pipeline-panel').forEach((p, i) => p.classList.toggle('active', i === idx));
  const panel = document.getElementById('panel-' + idx);
  if (panel) animateSteps(panel);
}

function animateSteps(panel) {
  const steps = panel.querySelectorAll('.pipeline-step');
  steps.forEach(s => s.classList.remove('show'));
  steps.forEach((s, i) => setTimeout(() => s.classList.add('show'), 120 * i));
}

/* ─── Scroll reveal (animos.app style) ────────────────────────── */
const io = new IntersectionObserver((entries) => {
  entries.forEach(e => { if (e.isIntersecting) { e.target.classList.add('visible'); io.unobserve(e.target); } });
}, { threshold: 0.12 });

/* ─── Animated counters (spade.com "0% → 95%" count-up-on-scroll) ─────── */
function animateCounter(el) {
  const target = parseFloat(el.dataset.target);
  if (Number.isNaN(target)) return;
  const decimals = parseInt(el.dataset.decimals || '0', 10);
  const prefix = el.dataset.prefix || '';
  const suffix = el.dataset.suffix || '';
  const duration = 1300;
  const startTime = performance.now();

  function tick(now) {
    const p = Math.min((now - startTime) / duration, 1);
    const eased = 1 - Math.pow(1 - p, 3); // ease-out-cubic, matches spade.com's snappy count-up
    const val = target * eased;
    el.textContent = prefix + val.toFixed(decimals) + suffix;
    if (p < 1) requestAnimationFrame(tick);
    else el.textContent = prefix + target.toFixed(decimals) + suffix;
  }
  requestAnimationFrame(tick);
}

const counterIo = new IntersectionObserver((entries) => {
  entries.forEach(e => {
    if (e.isIntersecting) { animateCounter(e.target); counterIo.unobserve(e.target); }
  });
}, { threshold: 0.4 });

/* ─── Kinetic tagline (spade.com "e v e r y l a y e r" spreading text) ── */
function makeKinetic(el) {
  const text = el.textContent;
  el.textContent = '';
  const spans = [];
  for (const ch of text) {
    const span = document.createElement('span');
    span.textContent = ch === ' ' ? '\u00A0' : ch;
    el.appendChild(span);
    spans.push(span);
  }
  const kineticIo = new IntersectionObserver((entries) => {
    entries.forEach(e => {
      if (e.isIntersecting) {
        spans.forEach((s, i) => setTimeout(() => { s.style.marginRight = '3px'; }, i * 16));
        kineticIo.unobserve(el);
      }
    });
  }, { threshold: 0.6 });
  kineticIo.observe(el);
}

/* ─── FAQ accordion ──────────────────────────────────── */
function toggleFaq(btn) {
  const item = btn.closest('.faq-item');
  const wasOpen = item.classList.contains('open');
  document.querySelectorAll('.faq-item').forEach(i => i.classList.remove('open'));
  if (!wasOpen) item.classList.add('open');
}

/* ─── Live proof feed simulation ───────────────────────────── */
const ROBOTS = ['robot-001', 'robot-002', 'robot-003', 'robot-004', 'robot-005'];
const ACTIONS = [
  { type: 'complete', label: 'task_complete', conf: () => 95 + Math.floor(Math.random() * 5) },
  { type: 'complete', label: 'task_complete', conf: () => 96 + Math.floor(Math.random() * 4) },
  { type: 'obstacle', label: 'obstacle_avoid', conf: () => 82 + Math.floor(Math.random() * 10) },
  { type: 'incident', label: 'incident_flag', conf: () => 70 + Math.floor(Math.random() * 8) },
];

function randHex(n) {
  const c = '0123456789abcdef';
  let s = '';
  for (let i = 0; i < n; i++) s += c[Math.floor(Math.random() * 16)];
  return s;
}

let proofCount = 142;
let xlmTotal = 38.4;
let zrepTotal = 1420;

function pushProof() {
  const feed = document.getElementById('proofFeed');
  if (!feed) return;
  const robot = ROBOTS[Math.floor(Math.random() * ROBOTS.length)];
  const action = ACTIONS[Math.floor(Math.random() * ACTIONS.length)];
  const conf = action.conf();

  const item = document.createElement('div');
  item.className = 'proof-item';
  item.style.opacity = '0';
  item.style.transform = 'translateY(-8px)';
  item.innerHTML =
    '<span class="proof-robot">' + robot + '</span>' +
    '<span class="proof-action ' + action.type + '">' + action.label + '</span>' +
    '<span class="proof-conf" style="font-size:11px;color:#a3a3a3">' + conf + '%</span>' +
    '<span class="proof-tx">G' + randHex(4).toUpperCase() + '…' + randHex(4).toUpperCase() + '</span>';
  feed.prepend(item);
  requestAnimationFrame(() => { item.style.transition = 'all .3s ease'; item.style.opacity = '1'; item.style.transform = 'translateY(0)'; });
  while (feed.children.length > 6) feed.removeChild(feed.lastChild);

  // update stats
  proofCount++;
  if (action.type === 'complete') xlmTotal += conf >= 95 ? 1.0 : 0.5;
  else if (action.type === 'obstacle') xlmTotal += 0.5;
  zrepTotal += 10;
  setText('statProofs', proofCount.toLocaleString());
  setText('statXlm', xlmTotal.toFixed(1));
  setText('statZrep', zrepTotal.toLocaleString());

  pushLog(robot, action, conf);
}

function setText(id, val) { const el = document.getElementById(id); if (el) el.textContent = val; }

function pushLog(robot, action, conf) {
  const log = document.getElementById('terminalLog');
  if (!log) return;
  const t = new Date().toLocaleTimeString('en-US', { hour12: false });
  const row = document.createElement('div');
  row.className = 'log-line';
  row.innerHTML =
    '<span class="log-time">[' + t + ']</span>' +
    '<span class="log-blue">PROOF</span>' +
    '<span class="' + (action.type === 'incident' ? 'log-yellow' : 'log-green') + '">' +
    robot + ' → ' + action.label + ' (' + conf + '%) ✓ verified on Stellar</span>';
  log.appendChild(row);
  while (log.children.length > 12) log.removeChild(log.firstChild);
  log.scrollTop = log.scrollHeight;
}

/* ─── Init ────────────────────────────────────────── */
document.addEventListener('DOMContentLoaded', () => {
  document.querySelectorAll('.reveal').forEach(el => io.observe(el));
  document.querySelectorAll('[data-target]').forEach(el => counterIo.observe(el));
  document.querySelectorAll('.kinetic-text').forEach(el => makeKinetic(el));

  const firstPanel = document.getElementById('panel-0');
  if (firstPanel) animateSteps(firstPanel);
  if (document.getElementById('proofFeed')) {
    pushProof(); pushProof(); pushProof();
    setInterval(pushProof, 2600);
  }
});
