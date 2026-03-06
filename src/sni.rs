use anyhow::Result;
use rama::{
    Context, Service,
    error::OpaqueError,
    tls::{
        boring::core::{
            pkey::{PKey, Private},
            x509::X509,
        },
        rustls::dep::{
            pki_types::{CertificateDer, PrivateKeyDer},
            rustls::{
                ServerConfig,
                crypto::aws_lc_rs::default_provider,
                server::{Acceptor, TlsStream},
            },
            tokio_rustls::{LazyConfigAcceptor,},
        },
    },
};
use std::{collections::HashMap, fmt, fs, path::PathBuf};
use std::{sync::Arc,};
use tokio::sync::{Mutex, OnceCell, RwLock};

use crate::{certs::{generate_ca_cert, generate_cert_for_host}, state::ProxyState};

pub fn generate_cert_for_host_rustls(
    ca_cert: &X509,
    ca_key: &PKey<Private>,
    hostname: &str,
) -> Result<(Vec<CertificateDer<'static>>, PrivateKeyDer<'static>), OpaqueError> {
    let (cert, key) =
        generate_cert_for_host(ca_cert, ca_key, hostname).map_err(|e| OpaqueError::from_std(e))?;

    let cert_der = cert.to_der().map_err(|e| OpaqueError::from_std(e))?;
    let key_der = key
        .private_key_to_der()
        .map_err(|e| OpaqueError::from_std(e))?;

    let cert_chain = vec![CertificateDer::from(cert_der)];
    let private_key = PrivateKeyDer::try_from(key_der)
        .map_err(|e| OpaqueError::from_display(format!("key conversion error: {}", e)))?;

    Ok((cert_chain, private_key))
}

pub struct AsyncTlsIssuerService<S> {
    inner: S,
    // Cache: Domain -> Mutex<Option<Arc<ServerConfig>>>
    // Mutex ensures we only generate once per domain even under concurrency
    state: Arc<ProxyState>
}

impl<S, State, IO> Service<State, IO> for AsyncTlsIssuerService<S>
where
    S: Service<State, TlsStream<IO>> ,
    IO: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
    State: Send + Sync + 'static,
    <S as Service<State, TlsStream<IO>>>::Error: fmt::Debug
{
    type Response = S::Response;
    type Error = OpaqueError;

    async fn serve(&self, ctx: Context<State>, io: IO) -> Result<Self::Response, Self::Error> {
        // Lazy Acceptor reads the TLS stream until ClientHello is received.
        let acceptor = LazyConfigAcceptor::new(Acceptor::default(), io);

        // yields back to the runtime until the handshake data arrives.
        let start = acceptor
            .await
            .map_err(|e| OpaqueError::from_display(format!("TLS accept error: {}", e)))?;

        let client_hello = start.client_hello();
        let domain = client_hello
            .server_name()
            .map(|s| s.to_string())
            .unwrap_or_else(|| "unknown".to_string());

        let config = self.resolve_config(&domain).await?;

        // call `into_stream` to upgrade the IO stream with the chosen config
        let tls_stream: TlsStream<IO> = start
            .into_stream(config)
            .await
            .map_err(|e| OpaqueError::from_display(format!("TLS handshake error: {}", e)))?;

        self.inner
            .serve(ctx, tls_stream)
            .await
            .map_err(|e| OpaqueError::from_display(format!("Inner service error: {:?}", e)))
    }
}

impl <S> AsyncTlsIssuerService<S> {
    pub fn new(inner: S, state: Arc<ProxyState>) -> Self {
        Self {
            inner,
            state,
        }
    }

    async fn resolve_config(&self, domain: &str) -> Result<Arc<ServerConfig>, OpaqueError> {

        let cell = self.state
            .tls_cache
            .entry(domain.to_string())
            .or_insert_with(|| Arc::new(OnceCell::new()))
            .clone();

        let config = cell
            .get_or_try_init(|| async {
                let cfg = generate_cert_config(
                    domain,
                    &self.state.ca_cert,
                    &self.state.ca_key,
                )?;
    
                Ok::<_, OpaqueError>(cfg)
            })
            .await?;

        Ok(Arc::new(config.clone()))

        // let entry = match entry_arc {
        //     Some(arc) => arc,
        //     None => {
        //         let mut write = self.cache.write().await;
        //         write
        //             .entry(domain.to_string())
        //             .or_insert_with(|| Arc::new(Mutex::new(None)))
        //             .clone()
        //     }
        // };

        // let mut guard = entry.lock().await;

        // if let Some(cfg) = &*guard {
        //     return Ok(cfg.clone());
        // }

        // tracing::info!("Generating cert for {}", domain);

        // let (cert_chain, key) = generate_cert_for_host_rustls(&self.ca_cert, &self.ca_key, domain)?;

        // let provider = Arc::new(default_provider());

        // *guard = Some(config.clone());

        // Ok(config)
    }
}

fn generate_cert_config (domain: &str, ca_cert: &X509, ca_key: &PKey<Private>) -> Result<ServerConfig, OpaqueError> {
    let (cert_chain, key) = generate_cert_for_host_rustls(&ca_cert, &ca_key, domain)?;
    let provider = Arc::new(default_provider());
    
    let builder = ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| OpaqueError::from_display(format!("protocol versions error: {}", e)))?
        .with_no_client_auth()
        .with_single_cert(cert_chain, key)
        .map_err(|e| OpaqueError::from_display(format!("invalid cert/key: {}", e)))?;

    Ok(builder)
}

pub fn load_ca(state_dir: &PathBuf) -> Result<(X509, PKey<Private>), Box<dyn std::error::Error>> {
    let cert_path = state_dir.join("ca.crt");
    let key_path = state_dir.join("ca.key");

    let cert_exists = cert_path.exists();
    let key_exists = key_path.exists();

    if !cert_exists || !key_exists {
        let (cert, key) = generate_ca_cert()?;

        fs::write(state_dir.join("ca.crt"), cert.to_pem()?)?;
        fs::write(state_dir.join("ca.key"), key.private_key_to_pem_pkcs8()?)?;

        return Ok((cert, key));
    }

    let ca_cert = X509::from_pem(&fs::read(cert_path)?)?;
    let ca_key = PKey::private_key_from_pem(&fs::read(key_path)?)?;

    Ok((ca_cert, ca_key))
}

#[tokio::main]
async fn main() {

//         let http_service = HttpServer::auto(exec).service(service_fn(http_service));

//         // Construct the Service Stack
//         let tls_service = AsyncTlsIssuerService::new(http_service, ca_cert, ca_key);

//         // Optional: Consume errors layer
//         let tcp_service = ConsumeErrLayer::default().into_layer(tls_service);

//         TcpListener::bind("127.0.0.1:64801")
//             .await
//             .expect("bind TCP Listener")
//             .serve_graceful(guard, tcp_service)
//             .await;
//     });

//     shutdown.shutdown_with_limit(Duration::from_secs(30)).await.unwrap();
}
