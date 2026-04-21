use dashmap::DashMap;
use directories::ProjectDirs;
use rama::tls::boring::core::pkey::{PKey, Private};
use rama::tls::boring::core::x509::X509;
use rama::tls::rustls::dep::rustls::ServerConfig;
use tokio::sync::OnceCell;
use std::fs;
use std::path::PathBuf;
use std::sync::{atomic::{AtomicU16, Ordering}, Arc};

use crate::routes::RouteManager;

pub(crate) trait AppStateTrait {
    fn get_routes(&self) -> Arc<RouteManager>;
    fn get_ca_cert(&self) -> &X509;
    fn get_ca_key(&self) -> &PKey<Private>;
    fn get_tls_cache(&self) -> &DashMap<String, Arc<OnceCell<ServerConfig>>>;
    fn get_proxy_port(&self) -> u16;
    fn set_proxy_port(&self, port: u16);
}

pub(crate) struct ProxyState {
    pub routes: Arc<RouteManager>,
    pub ca_cert: X509,
    pub ca_key: PKey<Private>,
    pub tls_cache: DashMap<String, Arc<OnceCell<ServerConfig>>>,
    pub proxy_port: AtomicU16,
}

pub(crate) type SharedState = Arc<ProxyState>;

pub(super) fn app_data_dir() -> PathBuf {
    let proj_dirs = ProjectDirs::from("you", "got", "deported")
        .expect("Could not determine project directories");

    proj_dirs.data_local_dir().to_path_buf()
}

pub(crate) fn init_app_storage() -> std::io::Result<()> {
    let dir = app_data_dir();

    fs::create_dir_all(&dir)?;

    let cert_path = dir.join("ca.crt");
    let key_path = dir.join("ca.key");

    println!("Cert: {:?}", cert_path);
    println!("Key: {:?}", key_path);

    Ok(())
}

impl AppStateTrait for ProxyState {
    fn get_routes(&self) -> Arc<RouteManager> {
        self.routes.clone()
    }

    fn get_ca_cert(&self) -> &X509 {
        &self.ca_cert
    }

    fn get_ca_key(&self) -> &PKey<Private> {
        &self.ca_key
    }

    fn get_tls_cache(&self) -> &DashMap<String, Arc<OnceCell<ServerConfig>>> {
        &self.tls_cache
    }

    fn get_proxy_port(&self) -> u16 {
        self.proxy_port.load(Ordering::Relaxed)
    }

    fn set_proxy_port(&self, port: u16) {
        self.proxy_port.store(port, Ordering::Relaxed);
    }
}