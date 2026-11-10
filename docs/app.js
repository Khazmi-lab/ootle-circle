// SPDX-License-Identifier: MIT
import { Circle, Ledger, Reject, STATUS_NAMES, RUNNING } from './engine.js';
const $ = (id) => document.getElementById(id);
const T = 1_000_000n; // micro-TARI per TARI
const fmt = (u) => { const s = (u / T).toString(); const f = (u % T).toString().padStart(6, '0').replace(/0+$/, ''); return f ? `${s}.${f}` : s; };
let ledger, circle, epoch = 0, names = [];

function act(fn, txLines) {
  try { const r = fn(); $('msg').textContent = ''; if (txLines) $('tx').textContent = txLines.join('\n'); render(); return r; }
  catch (e) { if (e instanceof Reject) { $('msg').innerHTML = `<span class="bad">Rejected by the template: ${e.message}</span>`; render(); } else throw e; }
}
function create() {
  ledger = new Ledger(); epoch = 1000;
  names = $('members').value.split(',').map((s) => s.trim()).filter(Boolean);
  const contribution = BigInt($('contribution').value) * T, collateral = BigInt($('collateral').value) * T;
  ['Organizer', ...names].forEach((n) => ledger.fund(n, 10_000n * T));
  const opts = { name: $('name').value, contribution, collateral, maxMembers: Math.max(2, names.length), roundEpochs: Number($('epochs').value) };
  act(() => {
    circle = new Circle(ledger, 'Organizer', opts);
    names.forEach((n) => circle.join(n, n, collateral));
    circle.start('Organizer', epoch);
  }, [...Circle.manifest('new', opts), '# each member:', ...Circle.manifest('join', { amount: fmt(collateral) }), '# organizer:', ...Circle.manifest('start')]);
}
function render() {
  if (!circle) return;
  const c = circle;
  $('stats').innerHTML = [
    ['Status', STATUS_NAMES[c.status]], ['Round', c.status === RUNNING ? `${c.round + 1} / ${c.members.length}` : '-'],
    ['Epoch', epoch], ['Due at epoch', c.status === RUNNING ? c.dueEpoch() : '-'], ['Pot', `${fmt(c.pot)} TARI`], ['Paid out', `${fmt(c.totalPaidOut)} TARI`],
  ].map(([k, v]) => `<div>${k}<b>${v}</b></div>`).join('');
  $('rows').innerHTML = c.members.map((m, i) => `<tr><td>${i + 1}</td><td>${m.key}</td><td>${fmt(ledger.balance(m.key))}</td>
    <td>${m.paidThisRound ? '<span class="ok">yes</span>' : 'no'}</td><td>${m.missed ? `<span class="bad">${m.missed}</span>` : 0}</td>
    <td>${fmt(m.collateralLeft)}</td><td>${i < c.round ? '<span class="pill">received</span>' : i === c.round && c.status === RUNNING ? '<span class="pill">this round</span>' : ''}</td>
    <td><button data-pay="${m.key}" ${m.paidThisRound || c.status !== RUNNING ? 'disabled' : ''}>Pay</button></td></tr>`).join('');
  document.querySelectorAll('[data-pay]').forEach((b) => b.onclick = () => pay(b.dataset.pay));
  $('log').textContent = c.events.map((e) => JSON.stringify(e, (k, v) => typeof v === 'bigint' ? `${fmt(v)} TARI` : v)).reverse().join('\n');
}
const pay = (who) => act(() => circle.contribute(who, circle.contribution), Circle.manifest('contribute', { account: who, amount: fmt(circle.contribution) }));
$('create').onclick = create;
$('settle').onclick = () => act(() => circle.settle(epoch), Circle.manifest('settle'));
$('tick').onclick = () => { epoch += 1; render(); };
$('deadline').onclick = () => { if (circle?.status === RUNNING) epoch = circle.dueEpoch(); render(); };
$('payall').onclick = () => circle.members.filter((m) => !m.paidThisRound).forEach((m) => pay(m.key));
create();
applyScene();

/** Optional walkthrough states for screenshots and the README: ?scene=paid|settled|missed */
function applyScene() {
  const scene = new URLSearchParams(location.search).get('scene') || location.hash.replace(/^#/, '');
  if (!circle || !scene) return;
  if (scene === 'paid') {
    circle.members.filter((m) => !m.paidThisRound).forEach((m) => pay(m.key));
    return;
  }
  if (scene === 'settled') {
    circle.members.filter((m) => !m.paidThisRound).forEach((m) => pay(m.key));
    act(() => circle.settle(epoch), Circle.manifest('settle'));
    return;
  }
  if (scene === 'missed') {
    // First two members pay; jump to the deadline so settle covers the rest from collateral.
    const paying = circle.members.slice(0, 2);
    paying.forEach((m) => pay(m.key));
    epoch = circle.dueEpoch();
    act(() => circle.settle(epoch), Circle.manifest('settle'));
  }
}
