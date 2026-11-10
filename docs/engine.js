// SPDX-License-Identifier: MIT
// A line-for-line mirror of template/src/lib.rs, used by the walletless demo and its tests.
// Amounts are BigInt micro-TARI, like the template's Amount.
export const OPEN = 0, RUNNING = 1, FINISHED = 2, CANCELLED = 3;
export const STATUS_NAMES = ['Open', 'Running', 'Finished', 'Cancelled'];

export class Reject extends Error {}
const ensure = (cond, msg) => { if (!cond) throw new Reject(msg); };

export class Ledger {
  constructor() { this.balances = new Map(); }
  fund(who, amount) { this.balances.set(who, (this.balances.get(who) ?? 0n) + BigInt(amount)); }
  balance(who) { return this.balances.get(who) ?? 0n; }
  withdraw(who, amount) {
    amount = BigInt(amount);
    ensure(this.balance(who) >= amount, 'insufficient funds');
    this.balances.set(who, this.balance(who) - amount);
    return amount;
  }
}

export class Circle {
  constructor(ledger, organizer, { name, contribution, collateral, maxMembers, roundEpochs }) {
    contribution = BigInt(contribution); collateral = BigInt(collateral);
    ensure(name.length > 0 && new TextEncoder().encode(name).length <= 64, 'name must be 1 to 64 bytes');
    ensure(contribution > 0n, 'contribution must be more than zero');
    ensure(collateral >= contribution, 'collateral must be at least one contribution');
    ensure(maxMembers >= 2 && maxMembers <= 32, 'a circle has 2 to 32 members');
    ensure(roundEpochs > 0, 'a round must last at least one epoch');
    Object.assign(this, { ledger, organizer, name, contribution, collateral, maxMembers, roundEpochs });
    this.status = OPEN; this.members = []; this.round = 0; this.startEpoch = 0;
    this.pot = 0n; this.collateralVault = 0n; this.totalPaidOut = 0n; this.events = [];
  }
  emit(name, data) { this.events.push({ name, ...data }); }
  owner(signer) { ensure(signer === this.organizer, 'Access Denied'); }
  positionOf(key) { const i = this.members.findIndex((m) => m.key === key); return i < 0 ? null : i; }
  dueEpoch() { return this.startEpoch + (this.round + 1) * this.roundEpochs; }
  send(to, amount) { if (amount > 0n) this.ledger.fund(to, amount); }

  join(signer, payoutTo, deposit) {
    deposit = BigInt(deposit);
    ensure(this.status === OPEN, 'the circle is not open for new members');
    ensure(this.members.length < this.maxMembers, 'the circle is full');
    ensure(deposit === this.collateral, 'deposit exactly the collateral');
    ensure(this.positionOf(signer) === null, 'this key is already a member');
    this.ledger.withdraw(signer, deposit);
    this.collateralVault += deposit;
    this.members.push({ key: signer, payoutTo, collateralLeft: this.collateral, paidThisRound: false, missed: 0, contributed: 0n });
    this.emit('Joined', { position: this.members.length - 1 });
    return this.members.length - 1;
  }
  leave(signer) {
    ensure(this.status === OPEN, 'members can only leave before the circle starts');
    const i = this.positionOf(signer); ensure(i !== null, 'not a member of this circle');
    const [m] = this.members.splice(i, 1);
    this.collateralVault -= m.collateralLeft; this.send(m.payoutTo, m.collateralLeft);
    this.emit('Left', {});
  }
  reorder(signer, order) {
    this.owner(signer);
    ensure(this.status === OPEN, 'the order is fixed once the circle starts');
    const n = this.members.length;
    ensure(order.length === n && new Set(order).size === n && order.every((p) => p >= 0 && p < n), 'order must list every member once');
    this.members = order.map((p) => this.members[p]);
    this.emit('Reordered', {});
  }
  start(signer, epoch) {
    this.owner(signer);
    ensure(this.status === OPEN, 'the circle has already started or ended');
    ensure(this.members.length >= 2, 'a circle needs at least 2 members');
    this.status = RUNNING; this.startEpoch = epoch;
    this.emit('Started', { members: this.members.length, startEpoch: epoch });
  }
  cancel(signer) {
    this.owner(signer);
    ensure(this.status === OPEN, 'only an open circle can be cancelled');
    for (const m of this.members) { this.collateralVault -= m.collateralLeft; this.send(m.payoutTo, m.collateralLeft); m.collateralLeft = 0n; }
    this.status = CANCELLED; this.emit('Cancelled', {});
  }
  contribute(signer, payment) {
    payment = BigInt(payment);
    ensure(this.status === RUNNING, 'the circle is not running');
    ensure(payment >= this.contribution, 'payment is less than the contribution');
    const i = this.positionOf(signer); ensure(i !== null, 'not a member of this circle');
    ensure(!this.members[i].paidThisRound, 'already paid this round');
    this.ledger.withdraw(signer, this.contribution); // the change never leaves the payer
    this.pot += this.contribution;
    const m = this.members[i]; m.paidThisRound = true; m.contributed += this.contribution;
    this.emit('Contributed', { round: this.round, position: i });
  }
  settle(epoch) {
    ensure(this.status === RUNNING, 'the circle is not running');
    const everyonePaid = this.members.every((m) => m.paidThisRound);
    ensure(everyonePaid || epoch >= this.dueEpoch(), 'the round is still open: not everyone has paid and the deadline has not passed');
    this.members.forEach((m, i) => {
      if (m.paidThisRound) return;
      const cover = m.collateralLeft < this.contribution ? m.collateralLeft : this.contribution;
      this.collateralVault -= cover; this.pot += cover;
      m.collateralLeft -= cover; m.missed += 1;
      this.emit('Missed', { round: this.round, position: i, covered: cover });
    });
    const amount = this.pot; const recipient = this.round;
    this.send(this.members[recipient].payoutTo, amount); this.pot = 0n; this.totalPaidOut += amount;
    this.emit('PotPaid', { round: this.round, position: recipient, amount });
    for (const m of this.members) m.paidThisRound = false;
    this.round += 1;
    if (this.round === this.members.length) {
      for (const m of this.members) { this.collateralVault -= m.collateralLeft; this.send(m.payoutTo, m.collateralLeft); m.collateralLeft = 0n; }
      this.status = FINISHED; this.emit('Finished', { totalPaidOut: this.totalPaidOut });
    }
    return amount;
  }
  /** Ootle transaction that the demo action maps to (shown in the Integrate panel). */
  static manifest(action, a = {}) {
    const w = (m) => `call_method(${a.account ?? 'ACCOUNT'}, "withdraw", [TARI, ${m}]) -> $bucket`;
    switch (action) {
      case 'new': return [`call_function(TEMPLATE, "new", ["${a.name}", ${a.contribution}, ${a.collateral}, ${a.maxMembers}u32, ${a.roundEpochs}u64])`];
      case 'join': return [w(a.amount), `call_method(CIRCLE, "join", [${a.account ?? 'ACCOUNT'}, $bucket])`];
      case 'contribute': return [w(a.amount), `call_method(CIRCLE, "contribute", [$bucket]) -> $change`, `call_method(${a.account ?? 'ACCOUNT'}, "deposit", [$change])`];
      default: return [`call_method(CIRCLE, "${action}", [${(a.args ?? []).join(', ')}])`];
    }
  }
}
