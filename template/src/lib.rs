// SPDX-License-Identifier: MIT
//! Ootle Circle: a rotating savings circle (ROSCA) on Tari Ootle.
//!
//! Hundreds of millions of people already save this way. It is called a jam'iya in Libya and
//! Egypt, a tontine in West Africa, a susu in Ghana and the Caribbean, a chama in Kenya, a
//! committee in Pakistan, a tanda in Mexico. Every member pays the same amount each round and one
//! member takes the whole pot, in an agreed order, until everyone has had a turn.
//!
//! The weak point of a paper circle is trust: the organizer holds the cash, and a member who has
//! already taken the pot can stop paying. This template removes the cash-holder and makes the
//! promise enforceable:
//!
//! * The pot sits in the component's vault. Only `settle` can move it, and only to the member
//!   whose turn it is, to the account that member named when joining.
//! * Every member locks a collateral deposit when joining. If a member misses a round, `settle`
//!   takes the missed contribution from that member's collateral, so the member whose turn it is
//!   still gets the full pot. With `collateral = contribution * (members - 1)` the circle is fully
//!   covered: nobody can walk away with more than they put in.
//! * Anyone can call `settle` once everyone has paid or the round's deadline epoch has passed,
//!   so the circle keeps moving even if the organizer disappears.
//! * When the last round settles, every member's remaining collateral goes back to them.
//!
//! Members are identified by the public key that signs their transactions, so a contribution
//! cannot be made in someone else's name.

use tari_template_lib::prelude::*;

/// One member of the circle.
#[derive(Debug, Clone, minicbor::Encode, minicbor::Decode, minicbor::CborLen)]
pub struct Member {
    /// The key that signs this member's transactions.
    #[n(0)]
    pub key: RistrettoPublicKeyBytes,
    /// Where this member's pot and returned collateral are sent.
    #[n(1)]
    pub payout_to: ComponentAddress,
    /// Collateral still locked for this member.
    #[n(2)]
    pub collateral_left: Amount,
    /// Has this member paid the current round?
    #[n(3)]
    pub paid_this_round: bool,
    /// Rounds this member missed (each one was covered from collateral, as far as it went).
    #[n(4)]
    pub missed: u32,
    /// Total this member has contributed, from their own pocket.
    #[n(5)]
    pub contributed: Amount,
}

pub const STATUS_OPEN: u8 = 0;
pub const STATUS_RUNNING: u8 = 1;
pub const STATUS_FINISHED: u8 = 2;
pub const STATUS_CANCELLED: u8 = 3;

pub const MIN_MEMBERS: u32 = 2;
pub const MAX_MEMBERS: u32 = 32;
pub const MAX_NAME_BYTES: usize = 64;

#[template]
mod savings_circle {
    use super::*;

    pub struct SavingsCircle {
        name: String,
        /// What every member pays every round, in micro-TARI.
        contribution: Amount,
        /// What every member locks when joining.
        collateral: Amount,
        max_members: u32,
        /// Length of one round, in epochs.
        round_epochs: u64,
        status: u8,
        /// Members in payout order: member `i` takes the pot in round `i`.
        members: Vec<Member>,
        /// Current round, 0-based. Equals the number of pots paid out so far.
        round: u32,
        /// Epoch at which round 0 started. Round `r` is due at `start_epoch + (r + 1) * round_epochs`.
        start_epoch: u64,
        pot: Vault,
        collateral_vault: Vault,
        total_paid_out: Amount,
    }

    impl SavingsCircle {
        /// Creates an open circle. The signer becomes the organizer, the only one who can
        /// reorder, start or cancel it. The organizer does not hold any funds.
        pub fn new(
            name: String,
            contribution: Amount,
            collateral: Amount,
            max_members: u32,
            round_epochs: u64,
        ) -> Component<Self> {
            assert!(!name.is_empty() && name.len() <= MAX_NAME_BYTES, "name must be 1 to 64 bytes");
            assert!(contribution.is_positive(), "contribution must be more than zero");
            assert!(
                collateral.to_u128() >= contribution.to_u128(),
                "collateral must be at least one contribution"
            );
            assert!(
                (MIN_MEMBERS..=MAX_MEMBERS).contains(&max_members),
                "a circle has 2 to 32 members"
            );
            assert!(round_epochs > 0, "a round must last at least one epoch");

            let access_rules = ComponentAccessRules::new()
                .method("join", rule![allow_all])
                .method("leave", rule![allow_all])
                .method("contribute", rule![allow_all])
                .method("settle", rule![allow_all])
                .method("info", rule![allow_all])
                .method("member", rule![allow_all]);
            // reorder, start and cancel fall back to the owner rule: organizer only.

            Component::new(Self {
                name,
                contribution,
                collateral,
                max_members,
                round_epochs,
                status: STATUS_OPEN,
                members: Vec::new(),
                round: 0,
                start_epoch: 0,
                pot: Vault::new_empty(TARI_TOKEN),
                collateral_vault: Vault::new_empty(TARI_TOKEN),
                total_paid_out: Amount::zero(),
            })
            .with_access_rules(access_rules)
            .create()
        }

        /// Joins the open circle with exactly `collateral` TARI. `payout_to` receives this
        /// member's pot and, at the end, whatever collateral is left. Returns the member's
        /// position, which is also their turn.
        pub fn join(&mut self, payout_to: ComponentAddress, deposit: Bucket) -> u32 {
            assert!(self.status == STATUS_OPEN, "the circle is not open for new members");
            assert!((self.members.len() as u32) < self.max_members, "the circle is full");
            assert!(deposit.resource_address() == TARI_TOKEN, "collateral must be in TARI");
            assert!(deposit.amount() == self.collateral, "deposit exactly the collateral");
            let key = CallerContext::transaction_signer_public_key();
            assert!(self.position_of(&key).is_none(), "this key is already a member");

            self.collateral_vault.deposit(deposit);
            self.members.push(Member {
                key,
                payout_to,
                collateral_left: self.collateral,
                paid_this_round: false,
                missed: 0,
                contributed: Amount::zero(),
            });
            let position = (self.members.len() - 1) as u32;
            emit_event("Joined", metadata![
                "circle" => self.name.clone(),
                "position" => position,
            ]);
            position
        }

        /// Leaves the circle before it starts and returns the collateral to the payout account.
        pub fn leave(&mut self) {
            assert!(self.status == STATUS_OPEN, "members can only leave before the circle starts");
            let key = CallerContext::transaction_signer_public_key();
            let i = self.position_of(&key).expect("not a member of this circle");
            let m = self.members.remove(i);
            self.send(&self.collateral_vault, m.payout_to, m.collateral_left);
            emit_event("Left", metadata!["circle" => self.name.clone()]);
        }

        /// Organizer only, before the start: sets the payout order. `order[k]` is the current
        /// position of the member who takes the pot in round `k`.
        pub fn reorder(&mut self, order: Vec<u32>) {
            assert!(self.status == STATUS_OPEN, "the order is fixed once the circle starts");
            let n = self.members.len();
            assert!(order.len() == n, "order must list every member once");
            let mut seen = vec![false; n];
            for &p in &order {
                let p = p as usize;
                assert!(p < n && !seen[p], "order must list every member once");
                seen[p] = true;
            }
            let old = core::mem::take(&mut self.members);
            self.members = order.iter().map(|&p| old[p as usize].clone()).collect();
            emit_event("Reordered", metadata!["circle" => self.name.clone()]);
        }

        /// Organizer only: closes membership and starts round 0 at the current epoch.
        pub fn start(&mut self) {
            assert!(self.status == STATUS_OPEN, "the circle has already started or ended");
            assert!(self.members.len() as u32 >= MIN_MEMBERS, "a circle needs at least 2 members");
            self.status = STATUS_RUNNING;
            self.start_epoch = Consensus::current_epoch();
            emit_event("Started", metadata![
                "circle" => self.name.clone(),
                "members" => self.members.len(),
                "start_epoch" => self.start_epoch,
            ]);
        }

        /// Organizer only, before the start: cancels the circle and returns every deposit.
        pub fn cancel(&mut self) {
            assert!(self.status == STATUS_OPEN, "only an open circle can be cancelled");
            for i in 0..self.members.len() {
                let (to, amount) = (self.members[i].payout_to, self.members[i].collateral_left);
                self.send(&self.collateral_vault, to, amount);
                self.members[i].collateral_left = Amount::zero();
            }
            self.status = STATUS_CANCELLED;
            emit_event("Cancelled", metadata!["circle" => self.name.clone()]);
        }

        /// Pays this round's contribution. The caller must be a member who has not paid this
        /// round. Anything above the contribution comes straight back as change.
        pub fn contribute(&mut self, mut payment: Bucket) -> Bucket {
            assert!(self.status == STATUS_RUNNING, "the circle is not running");
            assert!(payment.resource_address() == TARI_TOKEN, "contributions must be in TARI");
            assert!(
                payment.amount().to_u128() >= self.contribution.to_u128(),
                "payment is less than the contribution"
            );
            let key = CallerContext::transaction_signer_public_key();
            let i = self.position_of(&key).expect("not a member of this circle");
            assert!(!self.members[i].paid_this_round, "already paid this round");

            self.pot.deposit(payment.take(self.contribution));
            let m = &mut self.members[i];
            m.paid_this_round = true;
            m.contributed = Amount::from(m.contributed.to_u128() + self.contribution.to_u128());
            emit_event("Contributed", metadata![
                "circle" => self.name.clone(),
                "round" => self.round,
                "position" => i as u32,
            ]);
            payment
        }

        /// Closes the current round. Anyone can call it once every member has paid, or once the
        /// round's deadline epoch has passed. Missed contributions are taken from the late
        /// members' collateral, the whole pot goes to the member whose turn it is, and the next
        /// round opens. After the last round, all remaining collateral is returned.
        /// Returns the amount paid to this round's recipient.
        pub fn settle(&mut self) -> Amount {
            assert!(self.status == STATUS_RUNNING, "the circle is not running");
            let everyone_paid = self.members.iter().all(|m| m.paid_this_round);
            let due = self.round_due_epoch();
            assert!(
                everyone_paid || Consensus::current_epoch() >= due,
                "the round is still open: not everyone has paid and the deadline has not passed"
            );

            let contribution = self.contribution.to_u128();
            for i in 0..self.members.len() {
                if self.members[i].paid_this_round {
                    continue;
                }
                let left = self.members[i].collateral_left.to_u128();
                let cover = left.min(contribution);
                if cover > 0 {
                    self.pot.deposit(self.collateral_vault.withdraw(Amount::from(cover)));
                }
                let m = &mut self.members[i];
                m.collateral_left = Amount::from(left - cover);
                m.missed += 1;
                emit_event("Missed", metadata![
                    "circle" => self.name.clone(),
                    "round" => self.round,
                    "position" => i as u32,
                    "covered" => Amount::from(cover),
                ]);
            }

            let recipient = self.round as usize;
            let amount = self.pot.balance();
            let to = self.members[recipient].payout_to;
            self.send(&self.pot, to, amount);
            self.total_paid_out = Amount::from(self.total_paid_out.to_u128() + amount.to_u128());
            emit_event("PotPaid", metadata![
                "circle" => self.name.clone(),
                "round" => self.round,
                "position" => recipient as u32,
                "amount" => amount,
            ]);

            for m in self.members.iter_mut() {
                m.paid_this_round = false;
            }
            self.round += 1;

            if self.round as usize == self.members.len() {
                for i in 0..self.members.len() {
                    let (to, left) = (self.members[i].payout_to, self.members[i].collateral_left);
                    self.send(&self.collateral_vault, to, left);
                    self.members[i].collateral_left = Amount::zero();
                }
                self.status = STATUS_FINISHED;
                emit_event("Finished", metadata![
                    "circle" => self.name.clone(),
                    "total_paid_out" => self.total_paid_out,
                ]);
            }
            amount
        }

        /// Returns (name, status, round, member count, max members, contribution, collateral,
        /// round_epochs, start epoch, current round due epoch, pot balance, total paid out).
        #[allow(clippy::type_complexity)]
        pub fn info(&self) -> (String, u8, u32, u32, u32, Amount, Amount, u64, u64, u64, Amount, Amount) {
            (
                self.name.clone(),
                self.status,
                self.round,
                self.members.len() as u32,
                self.max_members,
                self.contribution,
                self.collateral,
                self.round_epochs,
                self.start_epoch,
                if self.status == STATUS_RUNNING { self.round_due_epoch() } else { 0 },
                self.pot.balance(),
                self.total_paid_out,
            )
        }

        /// Returns (payout account, paid this round, rounds missed, collateral left,
        /// total contributed, has received the pot) for the member at `position`.
        pub fn member(&self, position: u32) -> (ComponentAddress, bool, u32, Amount, Amount, bool) {
            let m = self.members.get(position as usize).expect("no member at that position");
            (
                m.payout_to,
                m.paid_this_round,
                m.missed,
                m.collateral_left,
                m.contributed,
                position < self.round,
            )
        }

        fn round_due_epoch(&self) -> u64 {
            self.start_epoch + (u64::from(self.round) + 1) * self.round_epochs
        }

        fn position_of(&self, key: &RistrettoPublicKeyBytes) -> Option<usize> {
            self.members.iter().position(|m| &m.key == key)
        }

        fn send(&self, from: &Vault, to: ComponentAddress, amount: Amount) {
            if amount.is_zero() {
                return;
            }
            let bucket = from.withdraw(amount);
            ComponentManager::get(to).invoke("deposit", args![bucket]);
        }
    }
}
