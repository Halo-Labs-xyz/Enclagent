//! Wallet-attestation and Eigen verification job artifacts for copytrading.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::agent::intent::{
    A2aExecutionIntentContract, ArtifactValidationError, ExecutionReceipt,
    RevenueShareSettlementReceipt, VerificationRecord, deterministic_hash,
};
use crate::secrets::{VerificationJobCredentialRef, VerificationJobProvider};

fn validate_hash(field: &'static str, value: &str) -> Result<(), ArtifactValidationError> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    {
        return Err(ArtifactValidationError::InvalidHash(field));
    }
    Ok(())
}

/// Wallet attestation receipt required before mirrored execution.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WalletAttestationReceipt {
    pub attestation_id: Uuid,
    pub wallet_address: String,
    pub signer_address: String,
    pub challenge_hash: String,
    pub signature_hash: String,
    pub verified_at: DateTime<Utc>,
}

impl WalletAttestationReceipt {
    pub fn validate(&self) -> Result<(), ArtifactValidationError> {
        if self.attestation_id.is_nil() {
            return Err(ArtifactValidationError::NilIdentifier("attestation_id"));
        }
        if self.wallet_address.trim().is_empty() {
            return Err(ArtifactValidationError::EmptyField("wallet_address"));
        }
        if self.signer_address.trim().is_empty() {
            return Err(ArtifactValidationError::EmptyField("signer_address"));
        }
        validate_hash("challenge_hash", &self.challenge_hash)?;
        validate_hash("signature_hash", &self.signature_hash)?;
        Ok(())
    }

    pub fn hash(&self) -> Result<String, serde_json::Error> {
        deterministic_hash(self)
    }
}

/// Lifecycle state for Eigen verification jobs.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VerificationJobStatus {
    Queued,
    Submitted,
    Verified,
    Failed,
}

/// Deterministic verification job metadata for EigenCloud / EigenCompute.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EigenVerificationJob {
    pub job_id: Uuid,
    pub provider: VerificationJobProvider,
    pub endpoint: String,
    pub request_hash: String,
    pub proof_lineage_hash: String,
    pub credentials: VerificationJobCredentialRef,
    pub status: VerificationJobStatus,
    pub created_at: DateTime<Utc>,
}

impl EigenVerificationJob {
    pub fn validate(&self) -> Result<(), ArtifactValidationError> {
        if self.job_id.is_nil() {
            return Err(ArtifactValidationError::NilIdentifier("job_id"));
        }
        if self.endpoint.trim().is_empty() {
            return Err(ArtifactValidationError::EmptyField("endpoint"));
        }
        validate_hash("request_hash", &self.request_hash)?;
        validate_hash("proof_lineage_hash", &self.proof_lineage_hash)?;
        self.credentials
            .validate()
            .map_err(|_| ArtifactValidationError::InvalidValue("credentials"))?;
        Ok(())
    }

    pub fn hash(&self) -> Result<String, serde_json::Error> {
        deterministic_hash(self)
    }
}

#[derive(Debug, Serialize)]
struct ProofLineageSeed<'a> {
    signal_hash: &'a str,
    intent_hash: &'a str,
    execution_receipt_hash: &'a str,
    verification_record_hash: Option<&'a str>,
    settlement_hash: Option<&'a str>,
}

/// Build deterministic proof lineage hash for WS-10 copytrading.
pub fn build_copytrade_proof_lineage_hash(
    signal_hash: &str,
    intent: &A2aExecutionIntentContract,
    receipt: &ExecutionReceipt,
    verification: Option<&VerificationRecord>,
    settlement: Option<&RevenueShareSettlementReceipt>,
) -> Result<String, serde_json::Error> {
    let intent_hash = deterministic_hash(intent)?;
    let receipt_hash = receipt.hash()?;
    let verification_hash = verification.map(|record| record.hash()).transpose()?;
    let settlement_hash = settlement.map(|record| record.hash()).transpose()?;

    deterministic_hash(&ProofLineageSeed {
        signal_hash,
        intent_hash: &intent_hash,
        execution_receipt_hash: &receipt_hash,
        verification_record_hash: verification_hash.as_deref(),
        settlement_hash: settlement_hash.as_deref(),
    })
}

#[derive(Debug, Serialize)]
struct VerificationJobSeed<'a> {
    provider: VerificationJobProvider,
    endpoint: &'a str,
    proof_lineage_hash: &'a str,
}

/// Build a queued Eigen verification job descriptor for mirrored execution proofs.
pub fn create_eigen_verification_job(
    provider: VerificationJobProvider,
    endpoint: impl Into<String>,
    proof_lineage_hash: impl Into<String>,
    credentials: VerificationJobCredentialRef,
) -> Result<EigenVerificationJob, ArtifactValidationError> {
    let endpoint = endpoint.into();
    let proof_lineage_hash = proof_lineage_hash.into();

    if endpoint.trim().is_empty() {
        return Err(ArtifactValidationError::EmptyField("endpoint"));
    }
    validate_hash("proof_lineage_hash", &proof_lineage_hash)?;
    credentials
        .validate()
        .map_err(|_| ArtifactValidationError::InvalidValue("credentials"))?;

    let seed = VerificationJobSeed {
        provider,
        endpoint: &endpoint,
        proof_lineage_hash: &proof_lineage_hash,
    };
    let request_hash = deterministic_hash(&seed)
        .map_err(|_| ArtifactValidationError::InvalidValue("verification_job_seed"))?;

    Ok(EigenVerificationJob {
        job_id: Uuid::new_v4(),
        provider,
        endpoint,
        request_hash,
        proof_lineage_hash,
        credentials,
        status: VerificationJobStatus::Queued,
        created_at: Utc::now(),
    })
}

#[cfg(test)]
mod tests {
    use rust_decimal_macros::dec;

    use super::*;
    use crate::agent::intent::{ExecutionMode, ExecutionSide};

    #[test]
    fn wallet_attestation_validation_rejects_bad_hash() {
        let receipt = WalletAttestationReceipt {
            attestation_id: Uuid::new_v4(),
            wallet_address: "0xwallet".to_string(),
            signer_address: "0xsigner".to_string(),
            challenge_hash: "short".to_string(),
            signature_hash: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
                .to_string(),
            verified_at: Utc::now(),
        };

        assert_eq!(
            receipt.validate().unwrap_err(),
            ArtifactValidationError::InvalidHash("challenge_hash")
        );
    }

    #[test]
    fn proof_lineage_hash_is_deterministic() {
        let intent = A2aExecutionIntentContract {
            execution_intent_id: Uuid::new_v4(),
            intent_id: Uuid::new_v4(),
            source_signal_hash: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
                .to_string(),
            policy_hash: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
                .to_string(),
            profile_hash: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
                .to_string(),
            wallet_attestation_hash:
                "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".to_string(),
            expected_notional: dec!(100),
            expected_leverage: dec!(1),
            created_at: Utc::now(),
        };
        let receipt = ExecutionReceipt {
            receipt_id: Uuid::new_v4(),
            intent_id: intent.intent_id,
            mode: ExecutionMode::Paper,
            symbol: "BTC-USD".to_string(),
            side: ExecutionSide::Buy,
            notional: dec!(100),
            price_ref: dec!(50000),
            simulated_fills: vec![],
            decision_hash: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
                .to_string(),
            created_at: Utc::now(),
        };

        let h1 = build_copytrade_proof_lineage_hash(
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
            &intent,
            &receipt,
            None,
            None,
        )
        .unwrap();
        let h2 = build_copytrade_proof_lineage_hash(
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
            &intent,
            &receipt,
            None,
            None,
        )
        .unwrap();
        assert_eq!(h1, h2);
    }
}
