use dashmap::DashMap;
use directories::ProjectDirs;
use rama::tls::boring::core::pkey::{PKey, Private};
use rama::tls::boring::core::x509::X509;
use rama::tls::rustls::dep::rustls::ServerConfig;
use tokio::sync::OnceCell;
use std::fs;
use std::path::PathBuf;

use std::sync::{Arc};

use crate::routes::RouteManager;

pub(crate) struct ProxyState {
    pub routes: Arc<RouteManager>,
    pub ca_cert: X509,
    pub ca_key: PKey<Private>,
    pub tls_cache: DashMap<String, Arc<OnceCell<ServerConfig>>>
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

// Ransom, Boldy James, Nicholas Craven - Salvation For The Wicked
// Ras Kass - Leopard Eats Face
// Herc Cut The Lights - SSG'98
// Sasha Keable - Act II
// Chris Crack - Too Late To Start Following The Rules Now
// Jill Scott - To Whom This May Concern
// Boldy James, Nicholas Craven - Manhunt
// 