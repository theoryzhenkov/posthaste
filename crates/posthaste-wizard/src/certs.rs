//! Pure-Rust TLS material for a provisioned node: a local CA and a server leaf
//! signed by it. We deliberately emit a **CA + leaf** (not a single self-signed
//! cert): rustls/webpki rejects a cert that is simultaneously the trust anchor
//! and the end-entity (`CaUsedAsEndEntity`), so a flat self-signed cert cannot
//! be both served by the daemon and trusted by the client. The client trusts
//! `ca.crt`; the daemon serves `leaf.crt` + `leaf.key`.
//!
//! @spec docs/eph/PLAN-L2-install-wizard

use rcgen::{
    BasicConstraints, CertificateParams, DnType, ExtendedKeyUsagePurpose, IsCa, KeyPair,
    KeyUsagePurpose,
};
use time::{Duration, OffsetDateTime};

/// Validity windows. Certs are dated from provisioning time (not rcgen's
/// effectively-never-expiring default) with a small backdate for clock skew.
const CLOCK_SKEW: Duration = Duration::days(1);
const CA_VALIDITY: Duration = Duration::days(3650); // ~10 years
const LEAF_VALIDITY: Duration = Duration::days(825); // ~27 months (leaf-cert hygiene cap)

/// PEM-encoded CA + leaf material for a node.
pub struct TlsMaterial {
    pub ca_cert_pem: String,
    pub ca_key_pem: String,
    pub leaf_cert_pem: String,
    pub leaf_key_pem: String,
}

/// Generate a local CA and a server leaf whose SANs cover `sans` (DNS names or
/// IP addresses, parsed by rcgen). `sans` must be non-empty; the first entry is
/// also used as the leaf's Common Name for human legibility.
pub fn generate(sans: &[String]) -> Result<TlsMaterial, String> {
    if sans.is_empty() {
        return Err("at least one SAN (hostname or IP) is required for the leaf".into());
    }

    // --- Local CA (self-signed, marked as a CA, can sign one cert) ---
    let mut ca_params =
        CertificateParams::new(Vec::<String>::new()).map_err(|e| format!("ca params: {e}"))?;
    ca_params
        .distinguished_name
        .push(DnType::CommonName, "Posthaste Local CA");
    ca_params.is_ca = IsCa::Ca(BasicConstraints::Constrained(0));
    ca_params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
    let now = OffsetDateTime::now_utc();
    ca_params.not_before = now - CLOCK_SKEW;
    ca_params.not_after = now + CA_VALIDITY;
    let ca_key = KeyPair::generate().map_err(|e| format!("ca key: {e}"))?;
    let ca_cert = ca_params
        .self_signed(&ca_key)
        .map_err(|e| format!("ca self-sign: {e}"))?;

    // --- Server leaf (end-entity, serverAuth), signed by the CA ---
    let mut leaf_params =
        CertificateParams::new(sans.to_vec()).map_err(|e| format!("leaf params: {e}"))?;
    leaf_params
        .distinguished_name
        .push(DnType::CommonName, sans[0].clone());
    leaf_params.is_ca = IsCa::NoCa;
    leaf_params.use_authority_key_identifier_extension = true;
    leaf_params.key_usages = vec![
        KeyUsagePurpose::DigitalSignature,
        KeyUsagePurpose::KeyEncipherment,
    ];
    leaf_params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    leaf_params.not_before = now - CLOCK_SKEW;
    leaf_params.not_after = now + LEAF_VALIDITY;
    let leaf_key = KeyPair::generate().map_err(|e| format!("leaf key: {e}"))?;
    let leaf_cert = leaf_params
        .signed_by(&leaf_key, &ca_cert, &ca_key)
        .map_err(|e| format!("leaf sign: {e}"))?;

    Ok(TlsMaterial {
        ca_cert_pem: ca_cert.pem(),
        ca_key_pem: ca_key.serialize_pem(),
        leaf_cert_pem: leaf_cert.pem(),
        leaf_key_pem: leaf_key.serialize_pem(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_ca_and_leaf_pems() {
        let m = generate(&["localhost".into(), "127.0.0.1".into()]).unwrap();
        assert!(m.ca_cert_pem.contains("BEGIN CERTIFICATE"));
        assert!(m.leaf_cert_pem.contains("BEGIN CERTIFICATE"));
        assert!(m.ca_key_pem.contains("PRIVATE KEY"));
        assert!(m.leaf_key_pem.contains("PRIVATE KEY"));
        // CA and leaf are distinct certs.
        assert_ne!(m.ca_cert_pem, m.leaf_cert_pem);
    }

    #[test]
    fn rejects_empty_sans() {
        assert!(generate(&[]).is_err());
    }
}
