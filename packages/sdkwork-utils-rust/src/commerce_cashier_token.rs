//! Signed, short-lived cashier capability tokens for anonymous (no-login) payment.
//!
//! The cashier QR code used to carry the bare order primary key and the merchant
//! trade number in plaintext. That was only safe because the H5 cashier route sat
//! behind a login wall. Anonymous payment inverts that assumption: anyone holding
//! the QR image can reach the cashier, so the URL must carry a *capability* that
//! proves nothing more than "this single order may be paid".
//!
//! Wire format:
//!
//! ```text
//! v1.<base64url(utf8 json claims)>.<base64url(hmac-sha256("v1.<claims>", tenant key))>
//! ```
//!
//! Design rules (see SECURITY_SPEC.md §1 and §1.1):
//!
//! - The signature domain includes the version prefix, so a `v2` payload cannot be
//!   replayed as `v1` and downgrade the verifier.
//! - Keys are per tenant. A caller MUST verify with the key of the tenant carried
//!   in the claims, and a key/claim tenant disagreement MUST fail closed.
//! - `audience` binds the token to the cashier surface; a token minted for another
//!   audience is rejected.
//! - Expiry is mandatory and is bounded by the caller to
//!   `min(order remaining lifetime, cashier TTL)`.
//!
//! The token is a bearer capability. It intentionally carries no user identity:
//! payment authorization is about the order, not about who the payer is.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::{Deserialize, Serialize};

use crate::crypto::{
    aes_gcm_decrypt, aes_gcm_encrypt, derive_aes_256_key, hmac_sha256_base64url, sha256_hash,
    verify_hmac_sha256_base64url,
};
use crate::encoding::base64url_decode;

/// Current token wire version. The prefix participates in the signature domain.
pub const CASHIER_TOKEN_VERSION: &str = "v1";

/// Audience required for cashier payment tokens.
pub const CASHIER_TOKEN_AUDIENCE: &str = "cashier";

/// Environment variable holding the deployment-wide cashier-token secret.
///
/// Deliberately **not** the payment provider-credential master key: the order and
/// IAM services must verify cashier tokens, and neither of them has any business
/// holding the key that decrypts PSP merchant secrets. Sharing that key would turn
/// every token verifier into a credential decryptor.
pub const CASHIER_TOKEN_SECRET_ENV: &str = "SDKWORK_COMMERCE_CASHIER_TOKEN_SECRET";

/// HKDF salt isolating cashier-token keys from every other derived key.
const CASHIER_TOKEN_KEY_SALT: &[u8] = b"sdkwork.commerce.cashier-token.v1";

/// Key epoch used by tokens that predate epoch rotation.
///
/// Epoch 1 is the original derivation (`tenant + purpose` only), so tokens minted
/// before this field existed keep verifying. Epochs above 1 add the epoch to the
/// HKDF info label, which rotates the key without changing the deployment secret.
pub const CASHIER_TOKEN_EPOCH_DEFAULT: i64 = 1;

/// Serde default for the `epoch` claim (legacy tokens have no field).
fn default_cashier_token_epoch() -> i64 {
    CASHIER_TOKEN_EPOCH_DEFAULT
}

/// Minimum master key material length accepted by [`derive_cashier_token_key`].
const MIN_MASTER_KEY_LEN: usize = 32;

/// Upper bound on the JSON payload size accepted before signature verification.
///
/// Bounded so a hostile token cannot force unbounded base64/JSON work before the
/// signature check rejects it.
const MAX_PAYLOAD_LEN: usize = 2048;

/// Claims carried by a cashier capability token.
///
/// `amount` is a minor-unit decimal string, matching the int64 wire contract
/// (API_SPEC.md §13.6): amounts are never floats.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CashierTokenClaims {
    /// Owning tenant. The verification key must belong to this tenant.
    pub tenant_id: String,
    /// Commerce order primary key the capability grants payment for.
    pub order_id: String,
    /// Merchant trade number bound to the order.
    pub out_trade_no: String,
    /// Minor-unit amount, e.g. `"6990"` for CNY 69.90.
    pub amount: String,
    /// ISO-4217 currency code.
    pub currency_code: String,
    /// Cashier scene (`checkout` / `virtual` / `recharge`), used for rendering only.
    pub scene: String,
    /// Audience the token was minted for.
    pub audience: String,
    /// Tenant key epoch the signature was derived from.
    ///
    /// The verifier derives the key from this claim, so a rotated tenant can
    /// verify older tokens until they expire while new tokens use the new epoch.
    /// Missing in legacy payloads, which default to [`CASHIER_TOKEN_EPOCH_DEFAULT`].
    #[serde(default = "default_cashier_token_epoch")]
    pub epoch: i64,
    /// Issued-at, unix seconds.
    pub issued_at: i64,
    /// Expiry, unix seconds.
    pub expires_at: i64,
    /// Unique token id, used for audit correlation and idempotency derivation.
    pub token_id: String,
}

/// Inputs required to mint cashier claims.
///
/// A named-field input keeps call sites readable and prevents the positional
/// mixups that a nine-argument constructor invites.
#[derive(Debug, Clone, Copy)]
pub struct CashierTokenIssue<'a> {
    /// Owning tenant.
    pub tenant_id: &'a str,
    /// Commerce order primary key.
    pub order_id: &'a str,
    /// Merchant trade number.
    pub out_trade_no: &'a str,
    /// Minor-unit amount string.
    pub amount: &'a str,
    /// ISO-4217 currency code.
    pub currency_code: &'a str,
    /// Cashier scene.
    pub scene: &'a str,
    /// Lifetime in seconds. The caller clamps this to
    /// `min(order remaining lifetime, cashier TTL)`.
    pub ttl_seconds: i64,
    /// Tenant key epoch to sign with. Callers resolve the tenant's current epoch
    /// from the payment settings store; `0`/negative is clamped to epoch 1.
    pub key_epoch: i64,
    /// Unique token id.
    pub token_id: &'a str,
}

impl CashierTokenClaims {
    /// Builds claims with the cashier audience and an expiry of
    /// `issue.ttl_seconds` from `issued_at`.
    #[must_use]
    pub fn issue(issue: &CashierTokenIssue<'_>, issued_at: i64) -> Self {
        Self {
            tenant_id: issue.tenant_id.to_owned(),
            order_id: issue.order_id.to_owned(),
            out_trade_no: issue.out_trade_no.to_owned(),
            amount: issue.amount.to_owned(),
            currency_code: issue.currency_code.to_owned(),
            scene: issue.scene.to_owned(),
            audience: CASHIER_TOKEN_AUDIENCE.to_owned(),
            epoch: issue.key_epoch.max(CASHIER_TOKEN_EPOCH_DEFAULT),
            issued_at,
            expires_at: issued_at.saturating_add(issue.ttl_seconds.max(0)),
            token_id: issue.token_id.to_owned(),
        }
    }

    /// True when `now_unix` is at or past the token expiry.
    #[must_use]
    pub fn is_expired_at(&self, now_unix: i64) -> bool {
        now_unix >= self.expires_at
    }
}

/// Verification failure for a cashier capability token.
///
/// Every variant collapses to the same externally observable result (an anonymous
/// caller must not learn whether a token is unknown, tampered or expired), but the
/// distinction stays available for server-side audit and tests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CashierTokenError {
    /// Token does not have the `v1.payload.signature` shape.
    Malformed,
    /// Version prefix is not the supported one.
    UnsupportedVersion,
    /// The payload is larger than [`MAX_PAYLOAD_LEN`].
    PayloadTooLarge,
    /// HMAC verification failed.
    InvalidSignature,
    /// The payload is not valid UTF-8 JSON, or a field is missing.
    InvalidPayload,
    /// Audience does not match the expected one.
    AudienceMismatch,
    /// Token has expired.
    Expired,
    /// A mandatory claim is empty.
    MissingClaim(&'static str),
    /// Master key material is shorter than [`MIN_MASTER_KEY_LEN`].
    WeakKeyMaterial,
    /// The sealed signing secret could not be opened (wrong wrap key, wrong
    /// tenant scope, or tampered ciphertext).
    SecretIntegrity,
    /// No key-encryption key is configured, so a stored signing secret cannot
    /// be sealed or opened.
    SecretUnavailable,
}

impl std::fmt::Display for CashierTokenError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Malformed => formatter.write_str("cashier token is malformed"),
            Self::UnsupportedVersion => formatter.write_str("cashier token version is unsupported"),
            Self::PayloadTooLarge => formatter.write_str("cashier token payload is too large"),
            Self::InvalidSignature => formatter.write_str("cashier token signature is invalid"),
            Self::InvalidPayload => formatter.write_str("cashier token payload is invalid"),
            Self::AudienceMismatch => formatter.write_str("cashier token audience mismatched"),
            Self::Expired => formatter.write_str("cashier token has expired"),
            Self::MissingClaim(claim) => write!(formatter, "cashier token claim {claim} is empty"),
            Self::WeakKeyMaterial => formatter.write_str("cashier token key material is too short"),
            Self::SecretIntegrity => {
                formatter.write_str("cashier token signing secret failed integrity")
            }
            Self::SecretUnavailable => {
                formatter.write_str("cashier token signing secret is unavailable")
            }
        }
    }
}

impl std::error::Error for CashierTokenError {}

/// Derives the tenant-scoped cashier-token signing key from deployment master key material.
///
/// The minting side (payment) and every verifying side (order app-api, IAM payment
/// OAuth) MUST call this function. An independent derivation on either side would
/// make every minted token unverifiable, which is the failure mode this
/// centralisation exists to prevent.
///
/// The HKDF salt isolates cashier-token keys from provider-credential keys derived
/// from the same master key, and the info label binds the key to exactly one tenant
/// (SECURITY_SPEC.md §1.1: a token must be verified with a key that belongs to the
/// tenant carried in the claims).
pub fn derive_cashier_token_key(
    master_key: &[u8],
    tenant_id: &str,
) -> Result<[u8; 32], CashierTokenError> {
    derive_cashier_token_key_with_epoch(master_key, tenant_id, CASHIER_TOKEN_EPOCH_DEFAULT)
}

/// Derives the tenant-scoped cashier-token signing key for one key epoch.
///
/// Epoch 1 keeps the original derivation byte for byte, so tokens minted before
/// epoch rotation still verify. Every later epoch adds `epoch=<n>` to the HKDF
/// info label: rotating a tenant's epoch changes its key without changing the
/// deployment secret, which is what makes the payment-center refresh instant and
/// restart-free.
pub fn derive_cashier_token_key_with_epoch(
    master_key: &[u8],
    tenant_id: &str,
    epoch: i64,
) -> Result<[u8; 32], CashierTokenError> {
    if master_key.len() < MIN_MASTER_KEY_LEN {
        return Err(CashierTokenError::WeakKeyMaterial);
    }
    let tenant_id = tenant_id.trim();
    if tenant_id.is_empty() {
        return Err(CashierTokenError::MissingClaim("tenantId"));
    }
    let epoch = epoch.max(CASHIER_TOKEN_EPOCH_DEFAULT);
    let info = if epoch == CASHIER_TOKEN_EPOCH_DEFAULT {
        format!("tenant={tenant_id};purpose=cashier-token")
    } else {
        format!("tenant={tenant_id};epoch={epoch};purpose=cashier-token")
    };
    Ok(derive_aes_256_key(
        master_key,
        CASHIER_TOKEN_KEY_SALT,
        info.as_bytes(),
    ))
}

/// Reads the key epoch from a token's (not yet verified) payload.
///
/// The epoch is only a key selector: the signature must still validate under the
/// key derived for that epoch, so a caller cannot choose a weaker key by editing
/// the field. Malformed payloads fall back to [`CASHIER_TOKEN_EPOCH_DEFAULT`];
/// verification rejects them anyway.
#[must_use]
pub fn peek_cashier_token_epoch(token: &str) -> i64 {
    let Some(encoded_payload) = token.split('.').nth(1) else {
        return CASHIER_TOKEN_EPOCH_DEFAULT;
    };
    // Bound the pre-verification decode the same way `verify_cashier_token`
    // bounds the payload, so a hostile oversized token cannot force unbounded
    // base64/JSON work before the signature check.
    if encoded_payload.is_empty() || encoded_payload.len() > MAX_PAYLOAD_LEN {
        return CASHIER_TOKEN_EPOCH_DEFAULT;
    }
    let Some(bytes) = base64url_decode(encoded_payload) else {
        return CASHIER_TOKEN_EPOCH_DEFAULT;
    };
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Peeked {
        #[serde(default = "default_cashier_token_epoch")]
        epoch: i64,
    }
    serde_json::from_slice::<Peeked>(&bytes)
        .map(|peeked| peeked.epoch.max(CASHIER_TOKEN_EPOCH_DEFAULT))
        .unwrap_or(CASHIER_TOKEN_EPOCH_DEFAULT)
}

/// Reads the tenant claim from a token's (not yet verified) payload.
///
/// The tenant is only a key selector: the signature must still validate under
/// the key derived for that tenant, so an edited tenant claim cannot pass. This
/// is the tenant counterpart of [`peek_cashier_token_epoch`].
#[must_use]
pub fn peek_cashier_token_tenant(token: &str) -> Option<String> {
    let encoded_payload = token.split('.').nth(1)?;
    if encoded_payload.is_empty() || encoded_payload.len() > MAX_PAYLOAD_LEN {
        return None;
    }
    let bytes = base64url_decode(encoded_payload)?;
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Peeked {
        tenant_id: String,
    }
    let peeked: Peeked = serde_json::from_slice(&bytes).ok()?;
    let tenant = peeked.tenant_id.trim().to_owned();
    if tenant.is_empty() {
        None
    } else {
        Some(tenant)
    }
}

/// Message signed to bind a payer openid to a cashier capability token.
///
/// Defined here because two independent services must agree on it byte for byte:
/// the IAM anonymous payment OAuth mints the binding, and the order app-api
/// verifies it before accepting `wechat_jsapi` on an anonymous cashier link.
#[must_use]
pub fn cashier_openid_binding_message(token_id: &str, openid: &str) -> String {
    format!("cashier-openid|{token_id}|{openid}")
}

/// Reads the deployment cashier-token secret from the environment.
///
/// - `Ok(None)` when the variable is unset or blank: the deployment keeps the
///   login-required cashier entry (the token's absence in the URL is the flag).
/// - `Err(WeakKeyMaterial)` when it is configured but shorter than
///   [`MIN_MASTER_KEY_LEN`]: a configured-but-broken secret must fail loudly rather
///   than silently degrade every cashier link back to the login wall.
pub fn cashier_token_secret_from_env() -> Result<Option<Vec<u8>>, CashierTokenError> {
    let Some(value) = std::env::var(CASHIER_TOKEN_SECRET_ENV).ok() else {
        return Ok(None);
    };
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    if trimmed.len() < MIN_MASTER_KEY_LEN {
        return Err(CashierTokenError::WeakKeyMaterial);
    }
    Ok(Some(trimmed.as_bytes().to_vec()))
}

/// HKDF salt isolating the database-secret wrap key from every other key.
const CASHIER_TOKEN_SECRET_SEAL_SALT: &[u8] = b"sdkwork.commerce.cashier-token.secret.v1";

/// Algorithm identifier stored next to a sealed cashier signing secret.
///
/// [`CASHIER_TOKEN_SECRET_ENV`] is the **key-encryption key** (KEK) for the
/// signing secrets the payment center generates and stores in
/// `commerce_payment_cashier_token_setting`; once a database secret exists the
/// deployment value is no longer used as the signing key itself.
pub const CASHIER_TOKEN_SECRET_SEAL_ALGORITHM: &str = "aes256gcm-hkdf-cashier-secret-v1";

/// A cashier signing secret sealed for exactly one tenant scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SealedCashierTokenSecret {
    /// `base64(nonce || AES-256-GCM ciphertext)`.
    pub ciphertext: String,
    /// Identifier of the wrap key that produced [`Self::ciphertext`].
    pub encryption_key_id: String,
    /// Algorithm identifier, [`CASHIER_TOKEN_SECRET_SEAL_ALGORITHM`].
    pub encryption_algorithm: String,
    /// SHA-256 of the plaintext, for audit/verification without decryption.
    pub fingerprint_sha256: String,
}

/// Returns the wrap-key identifier recorded with a sealed secret.
#[must_use]
pub fn cashier_token_secret_key_id(kek: &[u8]) -> String {
    format!("cashier-{}", &sha256_hash(kek)[..16])
}

/// Seals a generated signing secret for storage in the payment database.
///
/// The scope tenant is part of the HKDF info, so a ciphertext sealed for one
/// tenant cannot be opened under another tenant's scope even with the same KEK.
pub fn seal_cashier_token_secret(
    kek: &[u8],
    tenant_id: &str,
    secret: &[u8],
) -> Result<SealedCashierTokenSecret, CashierTokenError> {
    if secret.is_empty() {
        return Err(CashierTokenError::InvalidPayload);
    }
    let key = cashier_secret_wrap_key(kek, tenant_id)?;
    let ciphertext =
        aes_gcm_encrypt(&key, secret).map_err(|_| CashierTokenError::SecretIntegrity)?;
    Ok(SealedCashierTokenSecret {
        ciphertext,
        encryption_key_id: cashier_token_secret_key_id(kek),
        encryption_algorithm: CASHIER_TOKEN_SECRET_SEAL_ALGORITHM.to_owned(),
        fingerprint_sha256: sha256_hash(secret),
    })
}

/// Opens a signing secret sealed by [`seal_cashier_token_secret`].
///
/// Fails closed when the recorded key id or algorithm does not belong to this
/// deployment KEK, so a stolen database row cannot be replayed against another
/// installation.
pub fn open_cashier_token_secret(
    kek: &[u8],
    tenant_id: &str,
    ciphertext: &str,
    encryption_key_id: &str,
    encryption_algorithm: &str,
) -> Result<Vec<u8>, CashierTokenError> {
    if encryption_algorithm != CASHIER_TOKEN_SECRET_SEAL_ALGORITHM
        || encryption_key_id != cashier_token_secret_key_id(kek)
    {
        return Err(CashierTokenError::SecretUnavailable);
    }
    let key = cashier_secret_wrap_key(kek, tenant_id)?;
    aes_gcm_decrypt(&key, ciphertext).map_err(|_| CashierTokenError::SecretIntegrity)
}

fn cashier_secret_scope_tenant(tenant_id: &str) -> &str {
    let tenant = tenant_id.trim();
    if tenant.is_empty() {
        "0"
    } else {
        tenant
    }
}

fn cashier_secret_wrap_key(kek: &[u8], tenant_id: &str) -> Result<[u8; 32], CashierTokenError> {
    if kek.len() < MIN_MASTER_KEY_LEN {
        return Err(CashierTokenError::SecretUnavailable);
    }
    let tenant = cashier_secret_scope_tenant(tenant_id);
    let info = format!("tenant={tenant};purpose=cashier-token-secret");
    Ok(derive_aes_256_key(
        kek,
        CASHIER_TOKEN_SECRET_SEAL_SALT,
        info.as_bytes(),
    ))
}

/// Mints a cashier capability token.
///
/// `secret` is the tenant-scoped signing key. Callers in production MUST NOT share
/// one global key across tenants (SECURITY_SPEC.md §1.1).
pub fn encode_cashier_token(
    claims: &CashierTokenClaims,
    secret: &[u8],
) -> Result<String, CashierTokenError> {
    if secret.is_empty() {
        return Err(CashierTokenError::InvalidSignature);
    }
    validate_claims(claims)?;
    let payload = serde_json::to_vec(claims).map_err(|_| CashierTokenError::InvalidPayload)?;
    if payload.len() > MAX_PAYLOAD_LEN {
        return Err(CashierTokenError::PayloadTooLarge);
    }
    let encoded_payload = URL_SAFE_NO_PAD.encode(&payload);
    let signing_input = signing_input(CASHIER_TOKEN_VERSION, &encoded_payload);
    let signature = hmac_sha256_base64url(signing_input.as_bytes(), secret);
    Ok(format!(
        "{CASHIER_TOKEN_VERSION}.{encoded_payload}.{signature}"
    ))
}

/// Verifies a cashier capability token and returns its claims.
///
/// Verification order is fixed: shape, version, size, signature, payload, claims,
/// audience, expiry. Nothing is parsed or trusted before the signature check.
pub fn verify_cashier_token(
    token: &str,
    secret: &[u8],
    now_unix: i64,
    expected_audience: &str,
) -> Result<CashierTokenClaims, CashierTokenError> {
    if secret.is_empty() {
        return Err(CashierTokenError::InvalidSignature);
    }
    let mut parts = token.split('.');
    let (Some(version), Some(encoded_payload), Some(signature), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(CashierTokenError::Malformed);
    };
    if version != CASHIER_TOKEN_VERSION {
        return Err(CashierTokenError::UnsupportedVersion);
    }
    if encoded_payload.is_empty() || encoded_payload.len() > MAX_PAYLOAD_LEN {
        return Err(CashierTokenError::PayloadTooLarge);
    }
    let signing_input = signing_input(version, encoded_payload);
    let Ok(signature_bytes) =
        base64url_decode(signature).ok_or(CashierTokenError::InvalidSignature)
    else {
        return Err(CashierTokenError::InvalidSignature);
    };
    if !verify_hmac_sha256_base64url(signing_input.as_bytes(), secret, &signature_bytes) {
        return Err(CashierTokenError::InvalidSignature);
    }
    let payload = base64url_decode(encoded_payload).ok_or(CashierTokenError::InvalidPayload)?;
    let claims: CashierTokenClaims =
        serde_json::from_slice(&payload).map_err(|_| CashierTokenError::InvalidPayload)?;
    validate_claims(&claims)?;
    if claims.audience != expected_audience {
        return Err(CashierTokenError::AudienceMismatch);
    }
    if claims.is_expired_at(now_unix) {
        return Err(CashierTokenError::Expired);
    }
    Ok(claims)
}

/// Builds the signed signing input so the version prefix cannot be swapped.
fn signing_input(version: &str, encoded_payload: &str) -> String {
    format!("{version}.{encoded_payload}")
}

/// Rejects claims that would produce an unusable capability.
fn validate_claims(claims: &CashierTokenClaims) -> Result<(), CashierTokenError> {
    for (value, name) in [
        (claims.tenant_id.as_str(), "tenantId"),
        (claims.order_id.as_str(), "orderId"),
        (claims.out_trade_no.as_str(), "outTradeNo"),
        (claims.amount.as_str(), "amount"),
        (claims.currency_code.as_str(), "currencyCode"),
        (claims.token_id.as_str(), "tokenId"),
    ] {
        if value.trim().is_empty() {
            return Err(CashierTokenError::MissingClaim(name));
        }
    }
    if !claims.amount.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(CashierTokenError::MissingClaim("amount"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &[u8] = b"tenant-100001-cashier-signing-key";

    fn claims() -> CashierTokenClaims {
        CashierTokenClaims::issue(
            &CashierTokenIssue {
                tenant_id: "100001",
                order_id: "ORD-1",
                out_trade_no: "OT-1",
                amount: "6990",
                currency_code: "CNY",
                scene: "checkout",
                ttl_seconds: 900,
                key_epoch: 1,
                token_id: "jti-1",
            },
            1_700_000_000,
        )
    }

    #[test]
    fn epoch_one_keeps_the_legacy_key_derivation() {
        assert_eq!(
            derive_cashier_token_key_with_epoch(SECRET, "100001", 1).expect("epoch 1 key"),
            derive_cashier_token_key(SECRET, "100001").expect("legacy key")
        );
    }

    #[test]
    fn rotating_the_epoch_changes_the_tenant_key() {
        let epoch_one =
            derive_cashier_token_key_with_epoch(SECRET, "100001", 1).expect("epoch 1 key");
        let epoch_two =
            derive_cashier_token_key_with_epoch(SECRET, "100001", 2).expect("epoch 2 key");
        assert_ne!(epoch_one, epoch_two);
        // The same epoch must stay stable across calls.
        assert_eq!(
            epoch_two,
            derive_cashier_token_key_with_epoch(SECRET, "100001", 2).expect("epoch 2 key")
        );
    }

    #[test]
    fn a_token_carries_and_exposes_its_epoch() {
        let token = encode_cashier_token(
            &CashierTokenClaims::issue(
                &CashierTokenIssue {
                    tenant_id: "100001",
                    order_id: "ORD-1",
                    out_trade_no: "OT-1",
                    amount: "6990",
                    currency_code: "CNY",
                    scene: "checkout",
                    ttl_seconds: 900,
                    key_epoch: 3,
                    token_id: "jti-epoch-3",
                },
                1_700_000_000,
            ),
            &derive_cashier_token_key_with_epoch(SECRET, "100001", 3).expect("epoch 3 key"),
        )
        .expect("encode");

        assert_eq!(peek_cashier_token_epoch(&token), 3);
        let verified = verify_cashier_token(
            &token,
            &derive_cashier_token_key_with_epoch(SECRET, "100001", 3).expect("epoch 3 key"),
            1_700_000_100,
            CASHIER_TOKEN_AUDIENCE,
        )
        .expect("verify");
        assert_eq!(verified.epoch, 3);
        // The previous epoch's key must not verify the new token.
        assert_eq!(
            verify_cashier_token(
                &token,
                &derive_cashier_token_key_with_epoch(SECRET, "100001", 2).expect("epoch 2 key"),
                1_700_000_100,
                CASHIER_TOKEN_AUDIENCE
            ),
            Err(CashierTokenError::InvalidSignature)
        );
    }

    #[test]
    fn legacy_payload_without_epoch_defaults_to_one() {
        let payload = serde_json::json!({
            "tenantId": "100001",
            "orderId": "ORD-1",
            "outTradeNo": "OT-1",
            "amount": "6990",
            "currencyCode": "CNY",
            "scene": "checkout",
            "audience": CASHIER_TOKEN_AUDIENCE,
            "issuedAt": 1_700_000_000_i64,
            "expiresAt": 1_700_000_900_i64,
            "tokenId": "jti-legacy"
        });
        let claims: CashierTokenClaims = serde_json::from_value(payload).expect("legacy payload");
        assert_eq!(claims.epoch, CASHIER_TOKEN_EPOCH_DEFAULT);
    }

    #[test]
    fn round_trips_claims() {
        let token = encode_cashier_token(&claims(), SECRET).expect("encode");
        assert!(token.starts_with("v1."));
        let verified = verify_cashier_token(&token, SECRET, 1_700_000_100, CASHIER_TOKEN_AUDIENCE)
            .expect("verify");
        assert_eq!(verified, claims());
        assert_eq!(verified.expires_at, 1_700_000_900);
    }

    #[test]
    fn rejects_a_tampered_payload() {
        let token = encode_cashier_token(&claims(), SECRET).expect("encode");
        let mut parts: Vec<&str> = token.split('.').collect();
        let forged = URL_SAFE_NO_PAD.encode(br#"{"tenantId":"100001","amount":"1"}"#);
        parts[1] = &forged;
        let tampered = parts.join(".");
        assert_eq!(
            verify_cashier_token(&tampered, SECRET, 1_700_000_100, CASHIER_TOKEN_AUDIENCE),
            Err(CashierTokenError::InvalidSignature)
        );
    }

    #[test]
    fn rejects_a_token_signed_by_another_tenant_key() {
        let token = encode_cashier_token(&claims(), SECRET).expect("encode");
        assert_eq!(
            verify_cashier_token(
                &token,
                b"tenant-200002-cashier-signing-key",
                1_700_000_100,
                CASHIER_TOKEN_AUDIENCE
            ),
            Err(CashierTokenError::InvalidSignature)
        );
    }

    #[test]
    fn rejects_an_expired_token() {
        let token = encode_cashier_token(&claims(), SECRET).expect("encode");
        assert_eq!(
            verify_cashier_token(&token, SECRET, 1_700_000_900, CASHIER_TOKEN_AUDIENCE),
            Err(CashierTokenError::Expired)
        );
    }

    #[test]
    fn rejects_a_foreign_audience() {
        let token = encode_cashier_token(&claims(), SECRET).expect("encode");
        assert_eq!(
            verify_cashier_token(&token, SECRET, 1_700_000_100, "backend"),
            Err(CashierTokenError::AudienceMismatch)
        );
    }

    #[test]
    fn rejects_a_version_swap_and_malformed_shapes() {
        let token = encode_cashier_token(&claims(), SECRET).expect("encode");
        let swapped = token.replacen("v1.", "v2.", 1);
        assert_eq!(
            verify_cashier_token(&swapped, SECRET, 1_700_000_100, CASHIER_TOKEN_AUDIENCE),
            Err(CashierTokenError::UnsupportedVersion)
        );
        assert_eq!(
            verify_cashier_token("not-a-token", SECRET, 1_700_000_100, CASHIER_TOKEN_AUDIENCE),
            Err(CashierTokenError::Malformed)
        );
        assert_eq!(
            verify_cashier_token("v1..abc", SECRET, 1_700_000_100, CASHIER_TOKEN_AUDIENCE),
            Err(CashierTokenError::PayloadTooLarge)
        );
    }

    #[test]
    fn rejects_claims_without_a_bound_order() {
        let mut broken = claims();
        broken.order_id = "  ".to_owned();
        assert_eq!(
            encode_cashier_token(&broken, SECRET),
            Err(CashierTokenError::MissingClaim("orderId"))
        );
    }

    #[test]
    fn rejects_non_numeric_amounts() {
        let mut broken = claims();
        broken.amount = "69.90".to_owned();
        assert_eq!(
            encode_cashier_token(&broken, SECRET),
            Err(CashierTokenError::MissingClaim("amount"))
        );
    }

    #[test]
    fn rejects_an_empty_signing_key() {
        assert_eq!(
            encode_cashier_token(&claims(), b""),
            Err(CashierTokenError::InvalidSignature)
        );
        let token = encode_cashier_token(&claims(), SECRET).expect("encode");
        assert_eq!(
            verify_cashier_token(&token, b"", 1_700_000_100, CASHIER_TOKEN_AUDIENCE),
            Err(CashierTokenError::InvalidSignature)
        );
    }

    #[test]
    fn openid_binding_message_is_token_scoped() {
        assert_eq!(
            cashier_openid_binding_message("jti-1", "o_payer"),
            "cashier-openid|jti-1|o_payer"
        );
        assert_ne!(
            cashier_openid_binding_message("jti-1", "o_payer"),
            cashier_openid_binding_message("jti-2", "o_payer")
        );
    }

    #[test]
    fn derives_a_stable_tenant_scoped_key() {
        let master_key = [9_u8; 64];
        let key_a = derive_cashier_token_key(&master_key, "100001").expect("key a");
        let key_b = derive_cashier_token_key(&master_key, "100001").expect("key b");
        let key_other_tenant = derive_cashier_token_key(&master_key, "200002").expect("key c");
        assert_eq!(key_a, key_b, "the same master key and tenant must agree");
        assert_ne!(
            key_a, key_other_tenant,
            "keys must be bound to exactly one tenant"
        );
    }

    #[test]
    fn rejects_short_master_key_material() {
        assert_eq!(
            derive_cashier_token_key(&[0_u8; 16], "100001"),
            Err(CashierTokenError::WeakKeyMaterial)
        );
        assert_eq!(
            derive_cashier_token_key(&[0_u8; 64], "   "),
            Err(CashierTokenError::MissingClaim("tenantId"))
        );
    }

    #[test]
    fn signs_and_verifies_through_the_derived_tenant_key() {
        let master_key = [3_u8; 64];
        let signing_key = derive_cashier_token_key(&master_key, "100001").expect("signing key");
        let token = encode_cashier_token(&claims(), &signing_key).expect("encode");
        let verifying_key = derive_cashier_token_key(&master_key, "100001").expect("verify key");
        let verified = verify_cashier_token(
            &token,
            &verifying_key,
            1_700_000_100,
            CASHIER_TOKEN_AUDIENCE,
        )
        .expect("verify");
        assert_eq!(verified.order_id, "ORD-1");
    }

    #[test]
    fn a_token_minted_for_one_tenant_fails_another_tenants_key() {
        let master_key = [4_u8; 64];
        let tenant_key = derive_cashier_token_key(&master_key, "100001").expect("tenant key");
        let foreign_key = derive_cashier_token_key(&master_key, "200002").expect("foreign key");
        let token = encode_cashier_token(&claims(), &tenant_key).expect("encode");
        assert_eq!(
            verify_cashier_token(&token, &foreign_key, 1_700_000_100, CASHIER_TOKEN_AUDIENCE),
            Err(CashierTokenError::InvalidSignature)
        );
    }

    #[test]
    fn seals_and_opens_a_signing_secret_per_tenant() {
        let kek = [9_u8; 64];
        let sealed =
            seal_cashier_token_secret(&kek, "100001", b"generated-signing-secret").expect("seal");
        assert!(!sealed.ciphertext.contains("generated-signing-secret"));
        assert_eq!(
            sealed.encryption_algorithm,
            CASHIER_TOKEN_SECRET_SEAL_ALGORITHM
        );
        assert_eq!(sealed.encryption_key_id, cashier_token_secret_key_id(&kek));
        let opened = open_cashier_token_secret(
            &kek,
            "100001",
            &sealed.ciphertext,
            &sealed.encryption_key_id,
            &sealed.encryption_algorithm,
        )
        .expect("open");
        assert_eq!(opened, b"generated-signing-secret");
    }

    #[test]
    fn sealed_secret_is_scope_bound_to_its_tenant() {
        let kek = [10_u8; 64];
        let sealed = seal_cashier_token_secret(&kek, "100001", b"tenant-secret").expect("seal");
        assert_eq!(
            open_cashier_token_secret(
                &kek,
                "200002",
                &sealed.ciphertext,
                &sealed.encryption_key_id,
                &sealed.encryption_algorithm,
            ),
            Err(CashierTokenError::SecretIntegrity)
        );
        // The platform scope is the empty/`0` tenant.
        let platform =
            seal_cashier_token_secret(&kek, "0", b"platform-secret").expect("platform seal");
        assert!(open_cashier_token_secret(
            &kek,
            "",
            &platform.ciphertext,
            &platform.encryption_key_id,
            &platform.encryption_algorithm,
        )
        .is_ok());
    }

    #[test]
    fn a_foreign_key_or_algorithm_cannot_open_a_sealed_secret() {
        let kek = [11_u8; 64];
        let sealed = seal_cashier_token_secret(&kek, "100001", b"tenant-secret").expect("seal");
        let foreign = [12_u8; 64];
        assert_eq!(
            open_cashier_token_secret(
                &foreign,
                "100001",
                &sealed.ciphertext,
                &sealed.encryption_key_id,
                &sealed.encryption_algorithm,
            ),
            Err(CashierTokenError::SecretUnavailable)
        );
        assert_eq!(
            open_cashier_token_secret(
                &kek,
                "100001",
                &sealed.ciphertext,
                &sealed.encryption_key_id,
                "plaintext",
            ),
            Err(CashierTokenError::SecretUnavailable)
        );
        assert_eq!(
            seal_cashier_token_secret(&[1_u8; 16], "100001", b"short-kek"),
            Err(CashierTokenError::SecretUnavailable)
        );
    }
}
