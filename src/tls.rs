use std::{fs::File, io::BufReader, path::Path, sync::Arc};
use anyhow::{Context, Result, bail};
use rustls::{ClientConfig, RootCertStore, ServerConfig, pki_types::{CertificateDer, PrivateKeyDer, ServerName}, server::WebPkiClientVerifier};
use tokio_rustls::{TlsAcceptor, TlsConnector};

fn certificates(path: &Path) -> Result<Vec<CertificateDer<'static>>> {
    let file = File::open(path).with_context(|| format!("open certificate file {}", path.display()))?;
    let items = rustls_pemfile::certs(&mut BufReader::new(file))
        .collect::<std::result::Result<Vec<_>, _>>().context("parse PEM certificates")?;
    if items.is_empty() { bail!("certificate file contains no certificates"); }
    Ok(items)
}

fn private_key(path: &Path) -> Result<PrivateKeyDer<'static>> {
    let file = File::open(path).with_context(|| format!("open private key file {}", path.display()))?;
    rustls_pemfile::private_key(&mut BufReader::new(file))
        .context("parse PEM private key")?.context("no supported private key found")
}

fn roots(path: &Path) -> Result<RootCertStore> {
    let mut store = RootCertStore::empty();
    for cert in certificates(path)? { store.add(cert).context("add CA certificate")?; }
    Ok(store)
}

pub fn client_config(ca: &Path, cert: &Path, key: &Path) -> Result<Arc<ClientConfig>> {
    let config = ClientConfig::builder_with_protocol_versions(&[&rustls::version::TLS13])?
        .with_root_certificates(roots(ca)?)
        .with_client_auth_cert(certificates(cert)?, private_key(key)?)
        .context("build TLS 1.3 mutual-auth client configuration")?;
    Ok(Arc::new(config))
}

pub fn server_config(ca: &Path, cert: &Path, key: &Path) -> Result<TlsAcceptor> {
    let verifier = WebPkiClientVerifier::builder(Arc::new(roots(ca)?))
        .build().context("build client certificate verifier")?;
    let config = ServerConfig::builder_with_protocol_versions(&[&rustls::version::TLS13])?
        .with_client_cert_verifier(verifier)
        .with_single_cert(certificates(cert)?, private_key(key)?)
        .context("build TLS 1.3 mutual-auth server configuration")?;
    Ok(TlsAcceptor::from(Arc::new(config)))
}

pub fn server_name(value: &str) -> Result<ServerName<'static>> {
    ServerName::try_from(value.to_owned()).context("invalid TLS server name")
}

pub fn connector(config: Arc<ClientConfig>) -> TlsConnector { TlsConnector::from(config) }
