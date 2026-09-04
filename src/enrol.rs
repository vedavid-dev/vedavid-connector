//! Getting a certificate. The private key is generated here and never leaves
//! this process.

use crate::pb;
use crate::tunnel::channel_over;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName};
use std::sync::Arc;
use tokio_rustls::TlsConnector;

#[derive(Debug, thiserror::Error)]
pub enum EnrolError {
    #[error("generating a key: {0}")]
    Key(String),
    #[error("no enrolment token: {0}")]
    Token(String),
    #[error("trusting the relay: {0}")]
    Trust(String),
    #[error("reaching {addr}: {source}")]
    Dial {
        addr: String,
        source: std::io::Error,
    },
    #[error("the relay refused: {0}")]
    Refused(tonic::Status),
    #[error("the relay's answer was unusable: {0}")]
    Unusable(String),
    #[error("{0}")]
    Transport(String),
}

/// A certificate and the key it belongs to, plus the roots to trust from now on.
pub struct Identity {
    pub chain: Vec<CertificateDer<'static>>,
    pub key: PrivateKeyDer<'static>,
    pub roots: Vec<CertificateDer<'static>>,
    pub spiffe_id: String,
    pub relay_endpoints: Vec<String>,
}

impl Identity {
    pub fn key_clone(&self) -> PrivateKeyDer<'static> {
        self.key.clone_key()
    }
}

pub struct Bootstrap<'a> {
    pub relay_addr: &'a str,
    pub server_name: &'a str,
    /// The roots that vouch for the relay before enrolment has happened.
    pub roots_pem: &'a str,
    pub token: &'a str,
    pub build: pb::ConnectorBuild,
}

/// Asks the relay for a certificate; the request names no identity at all.
pub async fn enrol(b: Bootstrap<'_>) -> Result<Identity, EnrolError> {
    let mut params = rcgen::CertificateParams::new(Vec::<String>::new())
        .map_err(|e| EnrolError::Key(e.to_string()))?;
    params.distinguished_name = rcgen::DistinguishedName::new();
    let key = rcgen::KeyPair::generate().map_err(|e| EnrolError::Key(e.to_string()))?;
    let csr = params
        .serialize_request(&key)
        .map_err(|e| EnrolError::Key(e.to_string()))?
        .der()
        .to_vec();

    let tls = connect(b.relay_addr, b.server_name, b.roots_pem, None).await?;
    let channel = channel_over(tls)
        .await
        .map_err(|e| EnrolError::Transport(e.to_string()))?;

    let issued = pb::bootstrap_client::BootstrapClient::new(channel)
        .enroll(pb::EnrollRequest {
            enrollment_token: b.token.to_string(),
            csr_der: csr,
            build: Some(b.build),
        })
        .await
        .map_err(EnrolError::Refused)?
        .into_inner();

    if issued.cert_chain_der.is_empty() {
        return Err(EnrolError::Unusable("no certificate was returned".into()));
    }
    let key_der =
        PrivateKeyDer::try_from(key.serialize_der()).map_err(|e| EnrolError::Key(e.to_string()))?;

    Ok(Identity {
        chain: issued
            .cert_chain_der
            .into_iter()
            .map(CertificateDer::from)
            .collect(),
        key: key_der,
        roots: issued
            .trust_bundle_der
            .into_iter()
            .map(CertificateDer::from)
            .collect(),
        spiffe_id: issued.spiffe_id,
        relay_endpoints: issued.relay_endpoints,
    })
}

/// Dials the relay, presenting the certificate once there is one.
pub async fn connect(
    addr: &str,
    server_name: &str,
    roots_pem: &str,
    identity: Option<&Identity>,
) -> Result<tokio_rustls::client::TlsStream<tokio::net::TcpStream>, EnrolError> {
    let mut roots = rustls::RootCertStore::empty();
    for cert in rustls_pemfile::certs(&mut roots_pem.as_bytes()) {
        roots
            .add(cert.map_err(|e| EnrolError::Trust(e.to_string()))?)
            .map_err(|e| EnrolError::Trust(e.to_string()))?;
    }
    if let Some(id) = identity {
        for cert in &id.roots {
            let _ = roots.add(cert.clone());
        }
    }

    let builder = rustls::ClientConfig::builder().with_root_certificates(roots);
    let mut config = match identity {
        Some(id) => builder
            .with_client_auth_cert(id.chain.clone(), id.key_clone())
            .map_err(|e| EnrolError::Trust(e.to_string()))?,
        None => builder.with_no_client_auth(),
    };
    config.alpn_protocols = vec![b"h2".to_vec()];

    let tcp = tokio::net::TcpStream::connect(addr)
        .await
        .map_err(|source| EnrolError::Dial {
            addr: addr.to_string(),
            source,
        })?;
    let name: ServerName<'static> = server_name
        .to_string()
        .try_into()
        .map_err(|_| EnrolError::Trust(format!("{server_name} is not a valid server name")))?;
    TlsConnector::from(Arc::new(config))
        .connect(name, tcp)
        .await
        .map_err(|e| EnrolError::Transport(e.to_string()))
}
