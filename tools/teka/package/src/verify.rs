use anyhow::{ensure, Context, Result};
use openssl::x509::X509;
use std::{fs, path::Path};

#[derive(Default)]
pub struct Certificates {
    pub app: Vec<X509>,
    pub os: Vec<X509>,
}

impl Certificates {
    pub fn load(directory: &Path) -> Result<Self> {
        ensure!(directory.is_dir(), "trust directory does not exist");
        let load = |name: &str| -> Result<Vec<X509>> {
            let path = directory.join(name);
            match fs::read(&path) {
                Ok(bytes) => Ok(X509::stack_from_pem(&bytes)?),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
                Err(error) => Err(error)
                    .with_context(|| format!("read certificate bundle {}", path.display())),
            }
        };
        let certificates = Self {
            app: load("apps.pem")?,
            os: load("os.pem")?,
        };
        ensure!(
            !certificates.app.is_empty() || !certificates.os.is_empty(),
            "no configured role certificates"
        );
        Ok(certificates)
    }
}
