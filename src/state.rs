use directories::ProjectDirs;
use std::fs;
use std::path::PathBuf;

fn app_data_dir() -> PathBuf {
    let proj_dirs = ProjectDirs::from("you", "got", "deported")
        .expect("Could not determine project directories");

    proj_dirs.data_local_dir().to_path_buf()
}

pub(crate) fn init_app_storage() -> std::io::Result<()> {
    let dir = app_data_dir();

    fs::create_dir_all(&dir)?;

    let cert_path = dir.join("ca-cert.pem");
    let key_path = dir.join("ca-key.pem");

    println!("Cert: {:?}", cert_path);
    println!("Key: {:?}", key_path);

    Ok(())
}
