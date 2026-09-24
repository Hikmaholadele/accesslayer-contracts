//! Integration tests for deprecated key redemption.

mod contract_test_env;

use contract_test_env::{
    compute_expected_protocol_fee, register_creator_keys, register_test_creator, set_pricing_and_fees,
    test_env_with_auths,
};
use creator_keys::{events, ContractError, KeyStatus};
use soroban_sdk::{
    testutils::{Address as _, Events},
    Address, Env,
};

const KEY_PRICE: i128 = 1_000;
const CREATOR_BPS: u32 = 9_000;
const PROTOCOL_BPS: u32 = 1_000;

/// High protocol fee setup to ensure treasury has enough for redemption payouts.
// Creator gets 1 bps, protocol gets 9999 bps (99.99%).
const HIGH_PROTOCOL_CREATOR_BPS: u32 = 1;
const HIGH_PROTOCOL_PROTOCOL_BPS: u32 = 9_999;

fn setup<'a>(env: &'a Env) -> (creator_keys::CreatorKeysContractClient<'a>, Address) {
    let (client, _) = register_creator_keys(env);
    set_pricing_and_fees(env, &client, KEY_PRICE, CREATOR_BPS, PROTOCOL_BPS);
    let creator = register_test_creator(env, &client, "alice");
    (client, creator)
}

fn setup_high_protocol_fee<'a>(env: &'a Env) -> (creator_keys::CreatorKeysContractClient<'a>, Address) {
    let (client, _) = register_creator_keys(env);
    set_pricing_and_fees(
        env,
        &client,
        KEY_PRICE,
        HIGH_PROTOCOL_CREATOR_BPS,
        HIGH_PROTOCOL_PROTOCOL_BPS,
    );
    let creator = register_test_creator(env, &client, "alice");
    (client, creator)
}

fn buy_keys(
    client: &creator_keys::CreatorKeysContractClient<'_>,
    creator: &Address,
    buyer: &Address,
    count: u32,
) {
    for _ in 0..count {
        let quote = client.get_buy_quote(creator);
        client.buy_key(creator, buyer, &quote.total_amount, &None);
    }
}

#[test]
fn test_redeem_successful_payout() {
    let env = test_env_with_auths();
    let (client, creator) = setup_high_protocol_fee(&env);

    let holder1 = Address::generate(&env);
    let holder2 = Address::generate(&env);

    buy_keys(&client, &creator, &holder1, 5);
    buy_keys(&client, &creator, &holder2, 3);

    let protocol_fee_per_key = compute_expected_protocol_fee(KEY_PRICE, HIGH_PROTOCOL_PROTOCOL_BPS);
    let expected_treasury = protocol_fee_per_key * 8;

    client.deprecate_key(&creator);

    let payout = client.redeem(&holder1, &creator);
    let expected_payout = 5 * KEY_PRICE;

    assert_eq!(payout, expected_payout);
    assert_eq!(client.get_key_balance(&creator, &holder1), 0);
    assert_eq!(client.get_total_key_supply(&creator), 3);
    assert_eq!(client.get_creator_holder_count(&creator), 1);
    assert_eq!(client.get_treasury_balance(), expected_treasury - expected_payout);

    let event_log = env.events().all();
    let redeem_event = event_log.iter().find(|e| {
        let topics: soroban_sdk::Vec<soroban_sdk::Val> = e.1.clone();
        topics.get(0).map(|v| {
            let sym: soroban_sdk::Symbol = v.into_val(&env);
            sym == events::KEYS_REDEEMED_EVENT_NAME
        }).unwrap_or(false)
    }).expect("keys_redeemed event not found");

    let topics: soroban_sdk::Vec<soroban_sdk::Val> = redeem_event.1.clone();
    let event_name: soroban_sdk::Symbol = topics.get(0).unwrap().into_val(&env);
    let event_key_id: Address = topics.get(1).unwrap().into_val(&env);
    let event_wallet: Address = topics.get(2).unwrap().into_val(&env);

    assert_eq!(event_name, events::KEYS_REDEEMED_EVENT_NAME);
    assert_eq!(event_key_id, creator);
    assert_eq!(event_wallet, holder1);

    let payload: events::KeysRedeemedEvent = redeem_event.2.into_val(&env);
    assert_eq!(payload.wallet, holder1);
    assert_eq!(payload.key_id, creator);
    assert_eq!(payload.quantity, 5);
    assert_eq!(payload.payout_amount, expected_payout);
    assert_eq!(payload.ledger, env.ledger().sequence());
}

#[test]
fn test_redeem_non_deprecated_key_returns_error() {
    let env = test_env_with_auths();
    let (client, creator) = setup(&env);

    let holder = Address::generate(&env);
    buy_keys(&client, &creator, &holder, 5);

    let result = client.try_redeem(&holder, &creator);

    assert_eq!(result, Err(Ok(ContractError::KeyNotDeprecated)));
    assert_eq!(client.get_key_balance(&creator, &holder), 5);
    assert_eq!(client.get_total_key_supply(&creator), 5);
}

#[test]
fn test_redeem_zero_balance_returns_zero() {
    let env = test_env_with_auths();
    let (client, creator) = setup(&env);

    let holder = Address::generate(&env);

    client.deprecate_key(&creator);

    let payout = client.redeem(&holder, &creator);

    assert_eq!(payout, 0);
    assert_eq!(client.get_key_balance(&creator, &holder), 0);
}

#[test]
fn test_redeem_circulating_supply_reduction() {
    let env = test_env_with_auths();
    let (client, creator) = setup_high_protocol_fee(&env);

    let holder1 = Address::generate(&env);
    let holder2 = Address::generate(&env);

    buy_keys(&client, &creator, &holder1, 10);
    buy_keys(&client, &creator, &holder2, 5);

    client.deprecate_key(&creator);

    let supply_before = client.get_total_key_supply(&creator);
    assert_eq!(supply_before, 15);

    client.redeem(&holder1, &creator);
    let supply_after_h1 = client.get_total_key_supply(&creator);
    assert_eq!(supply_after_h1, 5);

    client.redeem(&holder2, &creator);
    let supply_after_h2 = client.get_total_key_supply(&creator);
    assert_eq!(supply_after_h2, 0);
}

#[test]
fn test_redeem_event_fields() {
    let env = test_env_with_auths();
    let (client, creator) = setup_high_protocol_fee(&env);

    let holder = Address::generate(&env);
    buy_keys(&client, &creator, &holder, 7);

    client.deprecate_key(&creator);

    client.redeem(&holder, &creator);

    let event_log = env.events().all();
    let redeem_event = event_log.iter().find(|e| {
        let topics: soroban_sdk::Vec<soroban_sdk::Val> = e.1.clone();
        topics.get(0).map(|v| {
            let sym: soroban_sdk::Symbol = v.into_val(&env);
            sym == events::KEYS_REDEEMED_EVENT_NAME
        }).unwrap_or(false)
    }).expect("keys_redeemed event not found");

    let payload: events::KeysRedeemedEvent = redeem_event.2.into_val(&env);

    assert_eq!(payload.wallet, holder);
    assert_eq!(payload.key_id, creator);
    assert_eq!(payload.quantity, 7);
    assert_eq!(payload.payout_amount, 7 * KEY_PRICE);
    assert_eq!(payload.ledger, env.ledger().sequence());

    let topics: soroban_sdk::Vec<soroban_sdk::Val> = redeem_event.1.clone();
    let event_name: soroban_sdk::Symbol = topics.get(0).unwrap().into_val(&env);
    let event_key_id: Address = topics.get(1).unwrap().into_val(&env);
    let event_wallet: Address = topics.get(2).unwrap().into_val(&env);

    assert_eq!(event_name, events::KEYS_REDEEMED_EVENT_NAME);
    assert_eq!(event_key_id, creator);
    assert_eq!(event_wallet, holder);
}

#[test]
fn test_redeem_multiple_holders() {
    let env = test_env_with_auths();
    let (client, creator) = setup_high_protocol_fee(&env);

    let holder1 = Address::generate(&env);
    let holder2 = Address::generate(&env);
    let holder3 = Address::generate(&env);

    buy_keys(&client, &creator, &holder1, 4);
    buy_keys(&client, &creator, &holder2, 6);
    buy_keys(&client, &creator, &holder3, 2);

    client.deprecate_key(&creator);

    let payout1 = client.redeem(&holder1, &creator);
    assert_eq!(payout1, 4 * KEY_PRICE);
    assert_eq!(client.get_key_balance(&creator, &holder1), 0);
    assert_eq!(client.get_total_key_supply(&creator), 8);

    let payout2 = client.redeem(&holder2, &creator);
    assert_eq!(payout2, 6 * KEY_PRICE);
    assert_eq!(client.get_key_balance(&creator, &holder2), 0);
    assert_eq!(client.get_total_key_supply(&creator), 2);

    let payout3 = client.redeem(&holder3, &creator);
    assert_eq!(payout3, 2 * KEY_PRICE);
    assert_eq!(client.get_key_balance(&creator, &holder3), 0);
    assert_eq!(client.get_total_key_supply(&creator), 0);
    assert_eq!(client.get_creator_holder_count(&creator), 0);
}

#[test]
fn test_redeem_insufficient_treasury_preserves_balance() {
    let env = test_env_with_auths();
    let (client, creator) = setup(&env);

    let holder = Address::generate(&env);
    buy_keys(&client, &creator, &holder, 100);

    client.deprecate_key(&creator);

    let treasury_before = client.get_treasury_balance();
    let expected_payout = 100 * KEY_PRICE;
    assert!(treasury_before < expected_payout, "test setup: treasury should be insufficient");

    let result = client.try_redeem(&holder, &creator);

    assert_eq!(result, Err(Ok(ContractError::InsufficientTreasuryBalance)));
    assert_eq!(client.get_key_balance(&creator, &holder), 100);
    assert_eq!(client.get_total_key_supply(&creator), 100);
    assert_eq!(client.get_treasury_balance(), treasury_before);
}

#[test]
fn test_redeem_second_redemption_pays_zero() {
    let env = test_env_with_auths();
    let (client, creator) = setup_high_protocol_fee(&env);

    let holder = Address::generate(&env);
    buy_keys(&client, &creator, &holder, 5);

    client.deprecate_key(&creator);

    let payout1 = client.redeem(&holder, &creator);
    assert_eq!(payout1, 5 * KEY_PRICE);

    let payout2 = client.redeem(&holder, &creator);
    assert_eq!(payout2, 0);
    assert_eq!(client.get_key_balance(&creator, &holder), 0);
}

#[test]
fn test_redeem_event_data_field_order() {
    assert_eq!(
        events::KEYS_REDEEMED_EVENT_DATA_FIELDS,
        ["wallet", "key_id", "quantity", "payout_amount", "ledger"]
    );
}