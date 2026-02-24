//! WS-8.1 integration coverage for onboarding, intent lifecycle, receipts,
//! verification fallback, and status artifacts.

use enclagent::agent::{
    ExecutionMode, ExecutionReceipt, ExecutionSide, IntentEnvelope, RiskLimitsPayload,
    SimulatedFill, StrategyPayload, VerificationBackend, VerificationRecord, VerificationStatus,
};
use rust_decimal_macros::dec;
use serde_json::json;

#[cfg(any(feature = "postgres", feature = "libsql"))]
use enclagent::setup::{SetupConfig, SetupWizard};

#[cfg(any(feature = "postgres", feature = "libsql"))]
#[test]
fn test_onboarding_step_extensions_in_full_mode() {
    let steps = SetupWizard::onboarding_step_titles(&SetupConfig::default());

    assert_eq!(
        steps,
        vec![
            "Database Connection",
            "Security",
            "Inference Provider",
            "Model Selection",
            "Embeddings (Semantic Search)",
            "Channel Configuration",
            "Background Tasks",
            "Hyperliquid Runtime Profile",
            "Wallet and Vault Policy",
            "Verification Backend",
        ]
    );
}

#[cfg(any(feature = "postgres", feature = "libsql"))]
#[test]
fn test_onboarding_step_extensions_in_channels_only_mode() {
    let steps = SetupWizard::onboarding_step_titles(&SetupConfig {
        skip_auth: true,
        channels_only: true,
    });

    assert_eq!(steps, vec!["Channel Configuration"]);
}

#[test]
fn test_intent_lifecycle_receipt_generation_and_verification_fallback() {
    let mut strategy = StrategyPayload::new();
    strategy.insert("name".to_string(), json!("mean_reversion"));
    strategy.insert("window".to_string(), json!(14));

    let mut risk_limits = RiskLimitsPayload::new();
    risk_limits.insert("max_notional".to_string(), json!(1000));

    let market_context_hash = blake3::hash(b"market-context-btc-usd").to_hex().to_string();
    let intent = IntentEnvelope::new(
        "agent-hl-1",
        "user-42",
        strategy,
        risk_limits,
        market_context_hash,
    );
    intent.validate().expect("intent should validate");

    let decision_hash = blake3::hash(b"decision:paper-buy-btc").to_hex().to_string();
    let receipt = ExecutionReceipt::new(
        intent.intent_id,
        ExecutionMode::Paper,
        "BTC-USD",
        ExecutionSide::Buy,
        dec!(250),
        dec!(50000),
        vec![SimulatedFill {
            quantity: dec!(0.005),
            price: dec!(50000),
        }],
        decision_hash.clone(),
    );
    receipt.validate().expect("receipt should validate");

    assert_eq!(receipt.intent_id, intent.intent_id);
    assert_eq!(receipt.decision_hash, decision_hash);
    assert_eq!(receipt.simulated_fills.len(), 1);
    assert!(receipt.created_at >= intent.created_at);

    let receipt_hash = receipt.hash().expect("receipt hash should serialize");
    assert_eq!(receipt_hash.len(), 64);
    assert!(
        receipt_hash
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    );

    let primary_failed = VerificationRecord::new(
        receipt.receipt_id,
        VerificationBackend::EigencloudPrimary,
        "eigen://verify/attempt-1",
        VerificationStatus::Failed,
    );
    primary_failed
        .validate()
        .expect("primary verification record should validate");

    let fallback_verified = VerificationRecord::new(
        receipt.receipt_id,
        VerificationBackend::SignedFallback,
        "fallback://receipt-chain/entry-1",
        VerificationStatus::Verified,
    );
    fallback_verified
        .validate()
        .expect("fallback verification record should validate");

    assert_eq!(primary_failed.receipt_id, receipt.receipt_id);
    assert_eq!(fallback_verified.receipt_id, receipt.receipt_id);
    assert_ne!(
        primary_failed.verification_id,
        fallback_verified.verification_id
    );
    assert!(fallback_verified.verified_at >= primary_failed.verified_at);
}
