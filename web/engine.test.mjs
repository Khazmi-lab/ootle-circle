// SPDX-License-Identifier: MIT
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { Circle, Ledger, Reject, FINISHED } from './engine.js';

const setup = (n, contribution, collateral, roundEpochs = 10) => {
  const ledger = new Ledger();
  const names = Array.from({ length: n }, (_, i) => `m${i}`);
  for (const who of ['org', ...names]) ledger.fund(who, 1_000_000);
  const c = new Circle(ledger, 'org', { name: 'Test', contribution, collateral, maxMembers: 8, roundEpochs });
  for (const m of names) c.join(m, m, collateral);
  return { ledger, c, names };
};
const total = (l) => [...l.balances.values()].reduce((a, b) => a + b, 0n);

test('full circle: everyone ends where they started, nothing is left in the component', () => {
  const { ledger, c, names } = setup(3, 1000, 2000);
  const before = total(ledger) + c.collateralVault;
  c.start('org', 100);
  for (let r = 0; r < 3; r++) { names.forEach((m) => c.contribute(m, 1000)); assert.equal(c.settle(100), 3000n); }
  names.forEach((m) => assert.equal(ledger.balance(m), 1_000_000n));
  assert.equal(c.status, FINISHED); assert.equal(c.collateralVault, 0n); assert.equal(c.pot, 0n);
  assert.equal(total(ledger), before);
});

test('missed payment is covered from collateral', () => {
  const { ledger, c } = setup(3, 1000, 2000);
  c.start('org', 100); c.contribute('m0', 1000); c.contribute('m1', 1000);
  assert.throws(() => c.settle(105), Reject);
  assert.equal(c.settle(110), 3000n);
  assert.equal(c.members[2].collateralLeft, 1000n); assert.equal(c.members[2].missed, 1);
  assert.equal(ledger.balance('m0'), 1_000_000n - 3000n + 3000n);
});

test('exhausted collateral shrinks the pot', () => {
  const { ledger, c } = setup(2, 1000, 1000, 5);
  c.start('org', 100); c.contribute('m0', 1000); c.settle(105);
  c.contribute('m0', 1000); assert.equal(c.settle(110), 1000n);
  assert.equal(ledger.balance('m0'), 1_000_000n); assert.equal(ledger.balance('m1'), 1_000_000n);
});

test('fixed deadline schedule', () => {
  const { c } = setup(2, 100, 100, 7);
  c.start('org', 100);
  assert.throws(() => c.settle(106), /still open/); c.settle(107);
  assert.throws(() => c.settle(113), /still open/); c.settle(114);
});

test('access and payment rules', () => {
  const { c } = setup(2, 1000, 1000);
  assert.throws(() => c.start('m0', 1), /Access Denied/);
  assert.throws(() => c.reorder('m0', [1, 0]), /Access Denied/);
  assert.throws(() => c.reorder('org', [0, 0]), /every member once/);
  c.reorder('org', [1, 0]); assert.equal(c.members[0].key, 'm1');
  c.start('org', 1);
  assert.throws(() => c.contribute('stranger', 1000), /not a member/);
  assert.throws(() => c.contribute('m0', 999), /less than the contribution/);
  c.contribute('m0', 1500); assert.throws(() => c.contribute('m0', 1000), /already paid/);
});

test('join, leave and cancel', () => {
  const ledger = new Ledger(); ['org', 'a', 'b', 'c'].forEach((w) => ledger.fund(w, 10_000));
  const c = new Circle(ledger, 'org', { name: 'x', contribution: 100, collateral: 300, maxMembers: 2, roundEpochs: 1 });
  assert.throws(() => c.join('a', 'a', 299), /exactly the collateral/);
  c.join('a', 'a', 300); assert.throws(() => c.join('a', 'a', 300), /already a member/);
  c.join('b', 'b', 300); assert.throws(() => c.join('c', 'c', 300), /full/);
  c.leave('b'); assert.equal(ledger.balance('b'), 10_000n);
  c.cancel('org'); assert.equal(ledger.balance('a'), 10_000n);
  assert.throws(() => c.start('org', 1), /already started or ended/);
});

test('bad parameters', () => {
  const l = new Ledger();
  const mk = (o) => () => new Circle(l, 'org', { name: 'x', contribution: 100, collateral: 100, maxMembers: 3, roundEpochs: 1, ...o });
  assert.throws(mk({ name: '' }), /name/); assert.throws(mk({ contribution: 0 }), /more than zero/);
  assert.throws(mk({ collateral: 99 }), /at least one contribution/); assert.throws(mk({ maxMembers: 1 }), /2 to 32/);
  assert.throws(mk({ maxMembers: 33 }), /2 to 32/); assert.throws(mk({ roundEpochs: 0 }), /one epoch/);
});
