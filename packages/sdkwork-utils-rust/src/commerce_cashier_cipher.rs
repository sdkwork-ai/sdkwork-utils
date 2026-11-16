//! Host-installed cipher for the database-stored anonymous-cashier signing secret.
//!
//! The payment center generates per-tenant signing secrets; they are stored
//! encrypted in `commerce_payment_cashier_token_setting`. The key that wraps
//! them (the KEK) must live outside the database, otherwise a database dump
//! could forge cashier tokens.
//!
//! This module is the extension point for that root key. A host installs one
//! [`CashierSigningKeyCipher`] at startup:
//!
//! - [`LocalFileCashierSigningKeyCipher`] wraps a deployment secret supplied by
//!   the container entrypoint / host installer (the current default);
//! - a Vault Transit or cloud KMS implementation can be installed later without
//!   touching the payment/order/IAM business crates.
//!
//! Business crates never read the deployment secret directly: they call the
//! installed cipher through [`cashier_signing_key_cipher`].

use std::sync::{Arc, OnceLock};

use crate::commerce_cashier_token::{
    cashier_token_secret_key_id, open_cashier_token_secret, seal_cashier_token_secret,
    CashierTokenError, SealedCashierTokenSecret, CASHIER_TOKEN_SECRET_ENV,
};

/// Minimum accepted KEK length, matching the token master-key requirement.
const MIN_KEK_LEN: usize = 32;

/// Wraps and unwraps database-stored cashier signing secrets.
pub trait CashierSigningKeyCipher: Send + Sync {
    /// Stable provider identifier shown by the admin surface (no key material).
    fn provider_id(&self) -> &'static str;

    /// Seals a generated signing secret for one tenant scope.
    fn seal(
        &self,
        tenant_id: &str,
        secret: &[u8],
    ) -> Result<SealedCashierTokenSecret, CashierTokenError>;

    /// Opens a stored signing secret for one tenant scope.
    fn open(
        &self,
        tenant_id: &str,
        ciphertext: &str,
        encryption_key_id: &str,
        encryption_algorithm: &str,
    ) -> Result<Vec<u8>, CashierTokenError>;
}

/// Default provider: the deployment secret is the key-encryption key.
///
/// The secret is supplied by the container entrypoint (`commerce-cashier-token.secret`
/// in the gateway-secrets volume) or by the host-native installer's
/// EnvironmentFile. Operators never edit it by hand.
#[derive(Clone)]
pub struct LocalFileCashierSigningKeyCipher {
    kek: Vec<u8>,
}

impl LocalFileCashierSigningKeyCipher {
    /// Loads the KEK from [`CASHIER_TOKEN_SECRET_ENV`].
    pub fn from_env() -> Result<Self, String> {
        let value = std::env::var(CASHIER_TOKEN_SECRET_ENV)
            .map_err(|_| format!("{CASHIER_TOKEN_SECRET_ENV} is not configured"))?;
        Self::from_key_material(value)
    }

    /// Builds a provider from explicit key material (tests / alternate hosts).
    pub fn from_key_material(kek: impl Into<Vec<u8>>) -> Result<Self, String> {
        let kek = kek.into();
        if kek.len() < MIN_KEK_LEN {
            return Err("cashier signing-key KEK is too short".to_owned());
        }
        Ok(Self { kek })
    }

    /// Key id recorded with sealed secrets (also used for diagnostics).
    #[must_use]
    pub fn key_id(&self) -> String {
        cashier_token_secret_key_id(&self.kek)
    }
}

impl CashierSigningKeyCipher for LocalFileCashierSigningKeyCipher {
    fn provider_id(&self) -> &'static str {
        "local-file"
    }

    fn seal(
        &self,
        tenant_id: &str,
        secret: &[u8],
    ) -> Result<SealedCashierTokenSecret, CashierTokenError> {
        seal_cashier_token_secret(&self.kek, tenant_id, secret)
    }

    fn open(
        &self,
        tenant_id: &str,
        ciphertext: &str,
        encryption_key_id: &str,
        encryption_algorithm: &str,
    ) -> Result<Vec<u8>, CashierTokenError> {
        open_cashier_token_secret(
            &self.kek,
            tenant_id,
            ciphertext,
            encryption_key_id,
            encryption_algorithm,
        )
    }
}

/// Provider installed when no KEK is available.
///
/// Minting then fails closed (`Ok(None)` at the caller) and verification rejects
/// every token, instead of silently falling back to another key.
#[derive(Debug, Clone, Copy, Default)]
pub struct UnavailableCashierSigningKeyCipher;

impl CashierSigningKeyCipher for UnavailableCashierSigningKeyCipher {
    fn provider_id(&self) -> &'static str {
        "unavailable"
    }

    fn seal(
        &self,
        _tenant_id: &str,
        _secret: &[u8],
    ) -> Result<SealedCashierTokenSecret, CashierTokenError> {
        Err(CashierTokenError::SecretUnavailable)
    }

    fn open(
        &self,
        _tenant_id: &str,
        _ciphertext: &str,
        _encryption_key_id: &str,
        _encryption_algorithm: &str,
    ) -> Result<Vec<u8>, CashierTokenError> {
        Err(CashierTokenError::SecretUnavailable)
    }
}

static CASHIER_SIGNING_KEY_CIPHER: OnceLock<Arc<dyn CashierSigningKeyCipher>> = OnceLock::new();

/// Installs the process-wide cipher. Idempotent across embedded assemblies.
pub fn install_cashier_signing_key_cipher(
    cipher: Arc<dyn CashierSigningKeyCipher>,
) -> Result<(), String> {
    if CASHIER_SIGNING_KEY_CIPHER.get().is_some() {
        return Ok(());
    }
    CASHIER_SIGNING_KEY_CIPHER
        .set(cipher)
        .map_err(|_| "cashier signing-key cipher is already initialized".to_owned())
}

/// Installs the deployment-secret provider, or an unavailable provider when no
/// KEK is configured.
///
/// Every host that can open cashier signing secrets (the composed gateway, the
/// payment/payment-standalone host, and standalone order/IAM hosts) calls this
/// once at startup.
pub fn install_local_file_cashier_signing_key_cipher_from_env() -> Result<(), String> {
    if cashier_signing_key_cipher_is_installed() {
        return Ok(());
    }
    let cipher: Arc<dyn CashierSigningKeyCipher> =
        match LocalFileCashierSigningKeyCipher::from_env() {
            Ok(cipher) => Arc::new(cipher),
            Err(_) => Arc::new(UnavailableCashierSigningKeyCipher),
        };
    install_cashier_signing_key_cipher(cipher)
}

/// True when a host installed a cipher (including the unavailable provider).
#[must_use]
pub fn cashier_signing_key_cipher_is_installed() -> bool {
    CASHIER_SIGNING_KEY_CIPHER.get().is_some()
}

/// Returns the installed cipher, or an error when the host never installed one.
pub fn cashier_signing_key_cipher() -> Result<Arc<dyn CashierSigningKeyCipher>, String> {
    CASHIER_SIGNING_KEY_CIPHER
        .get()
        .cloned()
        .ok_or_else(|| "cashier signing-key cipher is not installed by the host".to_owned())
}

/// Provider id for the admin read model (`unavailable` when not installed).
#[must_use]
pub fn cashier_signing_key_provider_id() -> &'static str {
    CASHIER_SIGNING_KEY_CIPHER
        .get()
        .map(|cipher| cipher.provider_id())
        .unwrap_or("unavailable")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_provider_round_trips_and_reports_its_id() {
        let cipher =
            LocalFileCashierSigningKeyCipher::from_key_material(vec![7_u8; 64]).expect("cipher");
        assert_eq!(cipher.provider_id(), "local-file");
        let sealed = cipher
            .seal("100001", b"generated-signing-secret")
            .expect("seal");
        let opened = cipher
            .open(
                "100001",
                &sealed.ciphertext,
                &sealed.encryption_key_id,
                &sealed.encryption_algorithm,
            )
            .expect("open");
        assert_eq!(opened, b"generated-signing-secret");
    }

    #[test]
    fn a_short_kek_is_rejected_and_the_unavailable_provider_fails_closed() {
        assert!(LocalFileCashierSigningKeyCipher::from_key_material(vec![1_u8; 8]).is_err());
        let unavailable = UnavailableCashierSigningKeyCipher;
        assert_eq!(unavailable.provider_id(), "unavailable");
        assert_eq!(
            unavailable.seal("100001", b"secret"),
            Err(CashierTokenError::SecretUnavailable)
        );
        assert_eq!(
            unavailable.open("100001", "ciphertext", "key", "alg"),
            Err(CashierTokenError::SecretUnavailable)
        );
    }
}
