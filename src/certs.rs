use std::{
    os::unix::fs::PermissionsExt as _, sync::Arc, time::{Duration, SystemTime}
};

use rama::{error::OpaqueError, tls::{boring::core::{
    asn1::{Asn1Integer, Asn1Time, Asn1TimeRef},
    bn::{BigNum, MsbOption},
    error::ErrorStack,
    hash::MessageDigest,
    nid::Nid,
    pkey::{Id, PKey, PKeyRef, Private},
    rsa::Rsa,
    x509::{
        X509, X509NameBuilder, X509Ref, X509Req, X509ReqBuilder, X509VerifyResult,
        extension::{self, SubjectKeyIdentifier},
    },
}, rustls::dep::{pki_types::{CertificateDer, PrivateKeyDer}, rustls::{crypto::aws_lc_rs, sign::CertifiedKey}}}};

const CA_KEY_FILE: &str = "ca-key.pem";
const CA_CERT_FILE: &str = "ca.pem";
const SERVER_KEY_FILE: &str = "server-key.pem";
const SERVER_CERT_FILE: &str = "server.pem";
const SERVER_VALIDITY_DAYS: u32 = 365;

/** Buffer (in ms) subtracted from expiry to trigger early regeneration. */
const EXPIRY_BUFFER_MS: i64 = 7 * 24 * 60 * 60 * 1000; // 7 days

const CA_COMMON_NAME: &str = "DEPORT LOCAL CA";

const OPENSSL_TIMEOUT_MS: i64 = 15_000;

const CA_VALIDITY_DAYS: u32 = 3650;

fn is_cert_expired(cert: &X509Ref) -> bool {
    // if let Ok(cert_data) = std::fs::read(cert_path) {
    // if let Ok(cert) = X509::from_pem(&cert_data) {
    let now = std::time::SystemTime::now();
    if let Ok(not_after) = asn1_time_to_system_time(cert.not_after()) {
        if let Ok(duration_until_expiry) = not_after.duration_since(now) {
            return duration_until_expiry.as_millis() < EXPIRY_BUFFER_MS as u128;
        }
    }
    // }
    // }
    true
}

pub fn generate_ca_cert() -> Result<(X509, PKey<Private>), ErrorStack> {
    let ca_key = Rsa::generate(2048)?;
    let mut ca_cert = X509::builder()?;
    ca_cert.set_version(2)?;
    let subject_name = {
        let mut name = X509NameBuilder::new()?;
        name.append_entry_by_text("CN", CA_COMMON_NAME)?;

        name.append_entry_by_text("C", "NG")?;
        name.append_entry_by_text("O", "DEPORT")?;
        name.build()
    };
    let not_before = Asn1Time::days_from_now(0)?;
    let not_after = Asn1Time::days_from_now(CA_VALIDITY_DAYS)?;

    let serial_number: Asn1Integer = {
        let mut serial = BigNum::new()?;
        serial.rand(128, MsbOption::MAYBE_ZERO, false)?;
        serial.to_asn1_integer()?
    };

    let key = PKey::from_rsa(ca_key.clone())?;

    let extension = extension::BasicConstraints::new().critical().ca().build()?;
    let extension2 = extension::KeyUsage::new()
        .critical()
        .key_cert_sign()
        .crl_sign()
        .build()?;

    ca_cert.set_serial_number(&serial_number)?;
    ca_cert.set_subject_name(&subject_name)?;
    ca_cert.set_issuer_name(&subject_name)?;
    ca_cert.set_pubkey(&key)?;
    ca_cert.set_not_before(&not_before)?;
    ca_cert.set_not_after(&not_after)?;
    ca_cert.append_extension(extension)?;
    ca_cert.append_extension(extension2)?;

    let subject_key_identifier =
        SubjectKeyIdentifier::new().build(&ca_cert.x509v3_context(None, None))?;
    ca_cert.append_extension(subject_key_identifier)?;

    ca_cert.sign(&key, MessageDigest::sha256())?;

    let cert = ca_cert.build();

    Ok((cert, key))

    // let _ = std::fs::write(CA_KEY_FILE, &ca_key.private_key_to_pem()?);
    // let _ = std::fs::write(CA_CERT_FILE, &ca_cert.build().to_pem()?);

    // let mut key_perm = match std::fs::metadata(key_path) {
    //     Ok(metadata) => metadata.permissions(),
    //     Err(_) => std::fs::Permissions::from_mode(0o600),
    // };
    // key_perm.set_mode(0o600);
    // std::fs::set_permissions(key_path, key_perm).ok();

    // let cert_perm = std::fs::metadata(cert_path).ok().map(|m| m.permissions());
    // if let Some(mut perm) = cert_perm {
    //     perm.set_mode(0o644);
    //     std::fs::set_permissions(cert_path, perm).ok();
    // }

    // let _ = fix_ownership(vec![key_path, cert_path]);
    // Ok(())
}

pub fn fix_ownership(paths: Vec<&str>) -> std::io::Result<()> {
    for path in paths {
        #[cfg(unix)]
        {
            let uid = nix::unistd::getuid();
            let gid = nix::unistd::getgid();
            let _ = nix::unistd::chown(path, Some(uid), Some(gid));
        }

        #[cfg(windows)]
        {
            let mut permissions = std::fs::metadata(path)?.permissions();
            permissions.set_readonly(false);
            std::fs::set_permissions(path, permissions)?;
        }
    }

    Ok(())
}

fn asn1_time_to_system_time(time: &Asn1TimeRef) -> Result<SystemTime, ErrorStack> {
    let unix_time = Asn1Time::from_unix(0)?.diff(time)?;
    Ok(SystemTime::UNIX_EPOCH
        + Duration::from_secs(unix_time.days as u64 * 86400 + unix_time.secs as u64))
}

fn make_request(key_pair: &PKey<Private>) -> Result<X509Req, ErrorStack> {
    let mut req_builder = X509ReqBuilder::new()?;
    req_builder.set_pubkey(key_pair)?;

    let mut x509_name = X509NameBuilder::new()?;
    x509_name.append_entry_by_text("C", "NG")?;
    x509_name.append_entry_by_text("O", "DEPORT")?;
    x509_name.append_entry_by_text("CN", CA_COMMON_NAME)?;
    let x509_name = x509_name.build();
    req_builder.set_subject_name(&x509_name)?;

    req_builder.sign(key_pair, MessageDigest::sha256())?;
    let req = req_builder.build();
    Ok(req)
}

/// Generate an arbitrary server cert -- could be either for our tld, or subdomains.
///
/// # Arguments
///
/// * `ca_cert` - Reference to default CA Cert
/// * `ca_key` - Reference to the CA Key
/// * `hosts` - Array of hosts with subject at 0-index.
fn generate_server_cert(
    ca_cert: &X509Ref,
    ca_key: &PKeyRef<Private>,
    hosts: Vec<String>,
) -> Result<(X509, PKey<Private>), ErrorStack> {
    // let ca_key_data = std::fs::read(ca_key_path).unwrap_or_default();
    // let ca_cert_data = std::fs::read(ca_cert_path).unwrap_or_default();

    // let ca_key = Rsa::private_key_from_pem(&ca_key_data)?;
    // let ca_cert = X509::from_pem(&ca_cert_data)?;

    let rsa = Rsa::generate(2048)?;
    let key = PKey::from_rsa(rsa)?;

    let req = make_request(&key)?;

    let mut server_cert_builder = X509::builder()?;
    server_cert_builder.set_version(2)?;

    let serial_number = {
        let mut serial = BigNum::new()?;
        serial.rand(128, MsbOption::MAYBE_ZERO, false)?;
        serial.to_asn1_integer()?
    };

    let subject_name = {
        let mut name = X509NameBuilder::new()?;
        name.append_entry_by_text("CN", hosts[0].as_str())?;
        name.build()
    };
    let not_before = Asn1Time::days_from_now(0)?;
    let not_after = Asn1Time::days_from_now(SERVER_VALIDITY_DAYS)?;

    let context = server_cert_builder.x509v3_context(Some(&ca_cert), None);

    let extension = extension::BasicConstraints::new().ca().build()?;
    let extension2 = extension::KeyUsage::new()
        .critical()
        .non_repudiation()
        .digital_signature()
        .key_encipherment()
        .build()?;

    let extension3 = hosts
        .into_iter()
        .fold(
            extension::SubjectAlternativeName::new(),
            |mut builder, host| {
                builder.dns(&host);
                builder
            },
        )
        .build(&context)?;
    // let extension3 = extension::SubjectAlternativeName::new()
    //     .dns(common_name)
    //     .dns("*.localhost")
    //     .build(&context)?;
    let extension4 = extension::ExtendedKeyUsage::new().server_auth().build()?;
    let extension5 = extension::AuthorityKeyIdentifier::new()
        .keyid(true)
        .issuer(true)
        .build(&context)?;
    let subject_key_identifier = extension::SubjectKeyIdentifier::new().build(&context)?;

    server_cert_builder.set_serial_number(&serial_number)?;
    server_cert_builder.set_subject_name(req.subject_name())?;
    server_cert_builder.set_issuer_name(ca_cert.subject_name())?;
    server_cert_builder.set_pubkey(&key)?;
    server_cert_builder.set_not_before(&not_before)?;
    server_cert_builder.set_not_after(&not_after)?;
    server_cert_builder.append_extension(extension)?;
    server_cert_builder.append_extension(extension2)?;
    server_cert_builder.append_extension(extension3)?;
    server_cert_builder.append_extension(extension4)?;
    server_cert_builder.append_extension(extension5)?;
    server_cert_builder.append_extension(subject_key_identifier)?;

    server_cert_builder.sign(ca_key, MessageDigest::sha256())?;
    let cert = server_cert_builder.build();
    Ok((cert, key))

    // let _ = std::fs::write(server_key_path, &server_key.private_key_to_pem()?);
    // let _ = std::fs::write(server_cert_path, &server_cert.build().to_pem()?);

    // let mut key_perm = match std::fs::metadata(server_key_path) {
    //     Ok(metadata) => metadata.permissions(),
    //     Err(_) => std::fs::Permissions::from_mode(0o600),
    // };
    // key_perm.set_mode(0o600);
    // std::fs::set_permissions(server_key_path, key_perm).ok();

    // let cert_perm = std::fs::metadata(server_cert_path)
    //     .ok()
    //     .map(|m| m.permissions());
    // if let Some(mut perm) = cert_perm {
    //     perm.set_mode(0o644);
    //     std::fs::set_permissions(server_cert_path, perm).ok();
    // };

    // let _ = fix_ownership(vec![server_key_path, server_cert_path]);

    // Ok(())
}

/// Verify that this cert was issued by this ca
fn verify_cert(ca_cert: &X509Ref, cert: &X509Ref) -> bool {
    match ca_cert.issued(&cert) {
        Ok(_) => true,
        Err(_) => false,
    }
}

pub(crate) fn is_cert_strong(cert: &X509Ref) -> bool {
    let sig_nid = cert.signature_algorithm().object().nid();

    let strong_hash = matches!(
        sig_nid,
        Nid::SHA256WITHRSAENCRYPTION
            | Nid::SHA384WITHRSAENCRYPTION
            | Nid::SHA512WITHRSAENCRYPTION
            | Nid::ECDSA_WITH_SHA256
            | Nid::ECDSA_WITH_SHA384
            | Nid::ECDSA_WITH_SHA512
    );

    if !strong_hash {
        return false;
    }

    let pub_key = match cert.public_key() {
        Ok(key) => key,
        Err(_) => return false,
    };

    match pub_key.id() {
        Id::RSA => {
            if let Ok(rsa) = pub_key.rsa() {
                return rsa.size() * 8 >= 2048;
            } else {
                return false;
            }
        }
        Id::EC => {
            if let Ok(ec) = pub_key.ec_key() {
                if let Some(curve) = ec.group().curve_name() {
                    matches!(
                        curve,
                        Nid::X9_62_PRIME256V1 | Nid::SECP521R1 | Nid::SECP384R1
                    )
                } else {
                    return false;
                }
            } else {
                return false;
            }
        }
        _ => return false,
    }
}

pub(crate) fn generate_cert_for_host(
    ca_cert: &X509Ref,
    ca_key: &PKeyRef<Private>,
    hostname: &str,
) -> Result<(X509, PKey<Private>), ErrorStack> {
    let hname_str = hostname.to_string();
    let mut hosts: Vec<String> = vec![hname_str];

    if let Some((_, rest)) = hostname.split_once('.') {
        if let Some(_) = rest.split_once('.') {
            let wildcard = format!("*.{}", rest);
            hosts.push(wildcard);
        }
    }

    generate_server_cert(ca_cert, ca_key, hosts)
}


pub fn bridge_boring_to_rustls(
    cert: &X509,
    key: &PKey<Private>,
) -> Result<CertifiedKey, OpaqueError> {
    let cert_der = cert.to_der().map_err(|e| OpaqueError::from_std(e))?;
    let key_der = key.private_key_to_der().map_err(|e| OpaqueError::from_std(e))?;

    let cert_chain = vec![CertificateDer::from(cert_der)];
    
    let private_key = PrivateKeyDer::try_from(key_der)
        .map_err(|e| OpaqueError::from_display(format!("key conversion error: {}", e)))?;

    let provider = Arc::new(aws_lc_rs::default_provider());
    
    let signing_key = provider
        .key_provider
        .load_private_key(private_key)
        .map_err(|e| OpaqueError::from_display(format!("load key error: {}", e)))?;

    Ok(CertifiedKey::new(cert_chain, signing_key))
}