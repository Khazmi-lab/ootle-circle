// SPDX-License-Identifier: MIT
//! Engine tests: every transaction runs the compiled WASM template through the same engine the
//! Ootle validators use (tari_template_test_tooling).
use tari_template_lib::{
    prelude::TARI_TOKEN,
    types::{Amount, ComponentAddress, NonFungibleAddress},
};
use tari_template_test_tooling::{
    TemplateTest,
    crypto::RistrettoSecretKey,
    engine_types::virtual_substate::{VirtualSubstate, VirtualSubstateId},
    support::assert_error::assert_reject_reason,
    transaction::args,
};

const TEMPLATE: &str = "SavingsCircle";
const START: u64 = 100;
const INITIAL: u64 = TemplateTest::FUNDED_ACCOUNT_INITIAL_BALANCE as u64;

struct Who {
    account: ComponentAddress,
    proof: NonFungibleAddress,
    key: RistrettoSecretKey,
}

fn who(test: &mut TemplateTest) -> Who {
    let (account, proof, key) = test.create_funded_account();
    Who { account, proof, key }
}

fn set_epoch(test: &mut TemplateTest, epoch: u64) {
    test.set_virtual_substate(VirtualSubstateId::CurrentEpoch, VirtualSubstate::CurrentEpoch(epoch));
}

fn tari(test: &TemplateTest, account: ComponentAddress) -> u128 {
    test.read_only_state_store()
        .get_vaults_for_account(account)
        .unwrap()
        .get(&TARI_TOKEN)
        .map(|v| v.balance().to_u128())
        .unwrap_or_default()
}

fn component_tari(test: &TemplateTest, c: ComponentAddress) -> u128 {
    let vaults = test.read_only_state_store().get_vaults_for_component(c).unwrap();
    vaults.values().map(|v| v.balance().to_u128()).sum()
}

/// Creates a circle signed by `org`. Returns its address.
fn create(test: &mut TemplateTest, org: &Who, contribution: u64, collateral: u64, max: u32, round_epochs: u64) -> ComponentAddress {
    let template = test.get_template_address(TEMPLATE);
    test.execute_expect_success(
        test.transaction()
            .call_function(template, "new", args![
                "Souq al-Juma jam'iya".to_string(),
                Amount::from(contribution),
                Amount::from(collateral),
                max,
                round_epochs
            ])
            .build_and_seal(&org.key),
        vec![org.proof.clone()],
    );
    test.expect_return::<ComponentAddress>(0)
}

fn create_fails(test: &mut TemplateTest, org: &Who, name: &str, contribution: u64, collateral: u64, max: u32, round_epochs: u64, reason: &str) {
    let template = test.get_template_address(TEMPLATE);
    let rejected = test.execute_expect_failure(
        test.transaction()
            .call_function(template, "new", args![
                name.to_string(),
                Amount::from(contribution),
                Amount::from(collateral),
                max,
                round_epochs
            ])
            .build_and_seal(&org.key),
        vec![org.proof.clone()],
    );
    assert_reject_reason(rejected, reason);
}

fn join(test: &mut TemplateTest, m: &Who, circle: ComponentAddress, deposit: u64) {
    test.execute_expect_success(
        test.transaction()
            .call_method(m.account, "withdraw", args![TARI_TOKEN, Amount::from(deposit)])
            .put_last_instruction_output_on_workspace("c")
            .call_method(circle, "join", args![m.account, Workspace("c")])
            .build_and_seal(&m.key),
        vec![m.proof.clone()],
    );
}

fn join_fails(test: &mut TemplateTest, m: &Who, circle: ComponentAddress, deposit: u64, reason: &str) {
    let rejected = test.execute_expect_failure(
        test.transaction()
            .call_method(m.account, "withdraw", args![TARI_TOKEN, Amount::from(deposit)])
            .put_last_instruction_output_on_workspace("c")
            .call_method(circle, "join", args![m.account, Workspace("c")])
            .build_and_seal(&m.key),
        vec![m.proof.clone()],
    );
    assert_reject_reason(rejected, reason);
}

fn call(test: &mut TemplateTest, by: &Who, circle: ComponentAddress, method: &str) {
    test.execute_expect_success(
        test.transaction().call_method(circle, method, args![]).build_and_seal(&by.key),
        vec![by.proof.clone()],
    );
}

fn call_fails(test: &mut TemplateTest, by: &Who, circle: ComponentAddress, method: &str, reason: &str) {
    let rejected = test.execute_expect_failure(
        test.transaction().call_method(circle, method, args![]).build_and_seal(&by.key),
        vec![by.proof.clone()],
    );
    assert_reject_reason(rejected, reason);
}

fn pay(test: &mut TemplateTest, m: &Who, circle: ComponentAddress, amount: u64) {
    test.execute_expect_success(
        test.transaction()
            .call_method(m.account, "withdraw", args![TARI_TOKEN, Amount::from(amount)])
            .put_last_instruction_output_on_workspace("p")
            .call_method(circle, "contribute", args![Workspace("p")])
            .put_last_instruction_output_on_workspace("change")
            .call_method(m.account, "deposit", args![Workspace("change")])
            .build_and_seal(&m.key),
        vec![m.proof.clone()],
    );
}

fn pay_fails(test: &mut TemplateTest, m: &Who, circle: ComponentAddress, amount: u64, reason: &str) {
    let rejected = test.execute_expect_failure(
        test.transaction()
            .call_method(m.account, "withdraw", args![TARI_TOKEN, Amount::from(amount)])
            .put_last_instruction_output_on_workspace("p")
            .call_method(circle, "contribute", args![Workspace("p")])
            .put_last_instruction_output_on_workspace("change")
            .call_method(m.account, "deposit", args![Workspace("change")])
            .build_and_seal(&m.key),
        vec![m.proof.clone()],
    );
    assert_reject_reason(rejected, reason);
}

/// Organizer plus `n` members who have joined an open circle (not started yet).
fn setup(n: usize, contribution: u64, collateral: u64, round_epochs: u64) -> (TemplateTest, Who, ComponentAddress, Vec<Who>) {
    let mut test = TemplateTest::my_crate();
    set_epoch(&mut test, START);
    let org = who(&mut test);
    let circle = create(&mut test, &org, contribution, collateral, 8, round_epochs);
    let members: Vec<Who> = (0..n).map(|_| who(&mut test)).collect();
    for m in &members {
        join(&mut test, m, circle, collateral);
    }
    (test, org, circle, members)
}

#[test]
fn a_full_circle_pays_everyone_once_and_returns_all_collateral() {
    let (mut test, org, circle, m) = setup(3, 1_000, 2_000, 10);
    assert_eq!(component_tari(&test, circle), 6_000);
    call(&mut test, &org, circle, "start");

    for round in 0..3 {
        for member in &m {
            pay(&mut test, member, circle, 1_000);
        }
        // Everyone paid, so the round can close before its deadline.
        test.execute_expect_success(
            test.transaction().call_method(circle, "settle", args![]).build_and_seal(&m[0].key),
            vec![],
        );
        let paid: Amount = test.expect_return(0);
        assert_eq!(paid.to_u128(), 3_000, "round {round} pot");
    }

    // Each member paid 3 x 1,000 and received one pot of 3,000; collateral came back.
    for member in &m {
        assert_eq!(tari(&test, member.account), INITIAL as u128);
    }
    assert_eq!(component_tari(&test, circle), 0);
    let status: u8 = test.extract_component_value(circle, "$.5");
    assert_eq!(status, 2, "finished");
    call_fails(&mut test, &m[0], circle, "settle", "the circle is not running");
}

#[test]
fn the_member_whose_turn_it_is_gets_the_pot_in_order() {
    let (mut test, org, circle, m) = setup(3, 500, 500, 10);
    call(&mut test, &org, circle, "start");
    for member in &m {
        pay(&mut test, member, circle, 500);
    }
    call(&mut test, &m[2], circle, "settle");
    // Round 0 belongs to position 0: paid 500 collateral + 500 contribution, got 1,500.
    assert_eq!(tari(&test, m[0].account), INITIAL as u128 - 1_000 + 1_500);
    assert_eq!(tari(&test, m[1].account), INITIAL as u128 - 1_000);
    assert_eq!(tari(&test, m[2].account), INITIAL as u128 - 1_000);
}

#[test]
fn a_missed_payment_is_covered_from_collateral_so_the_pot_stays_whole() {
    let (mut test, org, circle, m) = setup(3, 1_000, 2_000, 10);
    call(&mut test, &org, circle, "start");
    pay(&mut test, &m[0], circle, 1_000);
    pay(&mut test, &m[1], circle, 1_000);
    // m[2] does not pay. The round cannot close early...
    call_fails(&mut test, &m[0], circle, "settle", "the round is still open");
    // ...but anyone can close it once the deadline epoch has passed.
    set_epoch(&mut test, START + 10);
    let stranger = who(&mut test);
    call(&mut test, &stranger, circle, "settle");

    assert_eq!(tari(&test, m[0].account), INITIAL as u128 - 3_000 + 3_000, "full pot despite the missed payment");
    let (_, paid, missed, left, contributed, received): (ComponentAddress, bool, u32, Amount, Amount, bool) = {
        test.execute_expect_success(
            test.transaction().call_method(circle, "member", args![2u32]).build_and_seal(&m[0].key),
            vec![],
        );
        test.return_values()[0].decoded().unwrap()
    };
    assert!(!paid);
    assert_eq!(missed, 1);
    assert_eq!(left.to_u128(), 1_000);
    assert_eq!(contributed.to_u128(), 0);
    assert!(!received);
}

#[test]
fn when_collateral_runs_out_the_pot_shrinks_and_late_members_lose_their_deposit() {
    // Collateral covers exactly one missed round.
    let (mut test, org, circle, m) = setup(2, 1_000, 1_000, 5);
    call(&mut test, &org, circle, "start");
    // Round 0: m[0] pays, m[1] has already "planned" to skip; collateral covers it.
    pay(&mut test, &m[0], circle, 1_000);
    set_epoch(&mut test, START + 5);
    call(&mut test, &m[0], circle, "settle");
    assert_eq!(tari(&test, m[0].account), INITIAL as u128 - 2_000 + 2_000);
    // Round 1 is m[1]'s turn; m[0] pays, m[1] skips again, collateral is gone.
    pay(&mut test, &m[0], circle, 1_000);
    set_epoch(&mut test, START + 10);
    call(&mut test, &m[0], circle, "settle");
    // m[1] receives only what m[0] paid: 1,000. m[1] put in 1,000 collateral and lost it.
    assert_eq!(tari(&test, m[1].account), INITIAL as u128 - 1_000 + 1_000);
    // m[0]: paid 2 x 1,000 + 1,000 collateral, got 2,000 pot + 1,000 collateral back.
    assert_eq!(tari(&test, m[0].account), INITIAL as u128);
    assert_eq!(component_tari(&test, circle), 0);
}

#[test]
fn deadlines_follow_a_fixed_schedule() {
    let (mut test, org, circle, m) = setup(2, 100, 100, 7);
    call(&mut test, &org, circle, "start");
    set_epoch(&mut test, START + 6);
    call_fails(&mut test, &m[0], circle, "settle", "the round is still open");
    set_epoch(&mut test, START + 7);
    call(&mut test, &m[0], circle, "settle");
    // Round 1 is due at START + 14, not 7 epochs after the late settle.
    set_epoch(&mut test, START + 13);
    call_fails(&mut test, &m[0], circle, "settle", "the round is still open");
    set_epoch(&mut test, START + 14);
    call(&mut test, &m[0], circle, "settle");
}

#[test]
fn only_members_can_contribute_once_per_round_and_change_comes_back() {
    let (mut test, org, circle, m) = setup(2, 1_000, 1_000, 10);
    call(&mut test, &org, circle, "start");
    let outsider = who(&mut test);
    pay_fails(&mut test, &outsider, circle, 1_000, "not a member of this circle");
    pay_fails(&mut test, &m[0], circle, 999, "payment is less than the contribution");
    pay(&mut test, &m[0], circle, 1_500);
    assert_eq!(tari(&test, m[0].account), INITIAL as u128 - 2_000, "500 change returned");
    pay_fails(&mut test, &m[0], circle, 1_000, "already paid this round");
    assert_eq!(tari(&test, outsider.account), INITIAL as u128);
}

#[test]
fn only_the_organizer_can_reorder_start_or_cancel() {
    let (mut test, org, circle, m) = setup(2, 100, 100, 10);
    call_fails(&mut test, &m[0], circle, "start", "Access Denied");
    call_fails(&mut test, &m[0], circle, "cancel", "Access Denied");
    let rejected = test.execute_expect_failure(
        test.transaction().call_method(circle, "reorder", args![vec![1u32, 0u32]]).build_and_seal(&m[1].key),
        vec![m[1].proof.clone()],
    );
    assert_reject_reason(rejected, "Access Denied");
    call(&mut test, &org, circle, "start");
}

#[test]
fn reorder_sets_who_is_paid_first() {
    let (mut test, org, circle, m) = setup(3, 100, 100, 10);
    for bad in [vec![0u32, 1u32], vec![0u32, 0u32, 1u32], vec![0u32, 1u32, 3u32]] {
        let rejected = test.execute_expect_failure(
            test.transaction().call_method(circle, "reorder", args![bad]).build_and_seal(&org.key),
            vec![org.proof.clone()],
        );
        assert_reject_reason(rejected, "order must list every member once");
    }
    test.execute_expect_success(
        test.transaction().call_method(circle, "reorder", args![vec![2u32, 0u32, 1u32]]).build_and_seal(&org.key),
        vec![org.proof.clone()],
    );
    call(&mut test, &org, circle, "start");
    for member in &m {
        pay(&mut test, member, circle, 100);
    }
    call(&mut test, &m[0], circle, "settle");
    // The member who joined third now goes first.
    assert_eq!(tari(&test, m[2].account), INITIAL as u128 - 200 + 300);
    assert_eq!(tari(&test, m[0].account), INITIAL as u128 - 200);
}

#[test]
fn joining_rules_are_enforced() {
    let mut test = TemplateTest::my_crate();
    set_epoch(&mut test, START);
    let org = who(&mut test);
    let circle = create(&mut test, &org, 100, 300, 2, 10);
    let a = who(&mut test);
    let b = who(&mut test);
    let c = who(&mut test);
    join_fails(&mut test, &a, circle, 299, "deposit exactly the collateral");
    join(&mut test, &a, circle, 300);
    join_fails(&mut test, &a, circle, 300, "this key is already a member");
    call_fails(&mut test, &org, circle, "start", "a circle needs at least 2 members");
    join(&mut test, &b, circle, 300);
    join_fails(&mut test, &c, circle, 300, "the circle is full");
    call(&mut test, &org, circle, "start");
    call_fails(&mut test, &a, circle, "leave", "members can only leave before the circle starts");
    join_fails(&mut test, &c, circle, 300, "the circle is not open for new members");
}

#[test]
fn leaving_or_cancelling_before_the_start_returns_deposits() {
    let (mut test, org, circle, m) = setup(3, 100, 400, 10);
    call(&mut test, &m[1], circle, "leave");
    assert_eq!(tari(&test, m[1].account), INITIAL as u128);
    call_fails(&mut test, &m[1], circle, "leave", "not a member of this circle");
    call(&mut test, &org, circle, "cancel");
    assert_eq!(tari(&test, m[0].account), INITIAL as u128);
    assert_eq!(tari(&test, m[2].account), INITIAL as u128);
    assert_eq!(component_tari(&test, circle), 0);
    call_fails(&mut test, &org, circle, "start", "the circle has already started or ended");
}

#[test]
fn bad_circles_cannot_be_created() {
    let mut test = TemplateTest::my_crate();
    let org = who(&mut test);
    create_fails(&mut test, &org, "", 100, 100, 3, 10, "name must be 1 to 64 bytes");
    create_fails(&mut test, &org, "x", 0, 100, 3, 10, "contribution must be more than zero");
    create_fails(&mut test, &org, "x", 100, 99, 3, 10, "collateral must be at least one contribution");
    create_fails(&mut test, &org, "x", 100, 100, 1, 10, "a circle has 2 to 32 members");
    create_fails(&mut test, &org, "x", 100, 100, 33, 10, "a circle has 2 to 32 members");
    create_fails(&mut test, &org, "x", 100, 100, 3, 0, "a round must last at least one epoch");
}

#[test]
fn info_reports_the_circle() {
    let (mut test, org, circle, m) = setup(2, 250, 500, 4);
    call(&mut test, &org, circle, "start");
    pay(&mut test, &m[1], circle, 250);
    test.execute_expect_success(
        test.transaction().call_method(circle, "info", args![]).build_and_seal(&m[0].key),
        vec![],
    );
    #[allow(clippy::type_complexity)]
    let info: (String, u8, u32, u32, u32, Amount, Amount, u64, u64, u64, Amount, Amount) =
        test.return_values()[0].decoded().unwrap();
    assert_eq!(info.0, "Souq al-Juma jam'iya");
    assert_eq!((info.1, info.2, info.3, info.4), (1, 0, 2, 8));
    assert_eq!((info.5.to_u128(), info.6.to_u128()), (250, 500));
    assert_eq!((info.7, info.8, info.9), (4, START, START + 4));
    assert_eq!((info.10.to_u128(), info.11.to_u128()), (250, 0));
}
