use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};

use reqwest::blocking::ClientBuilder;
use reqwest::{Certificate, Identity};
use rpmrepo_metadata::{Checksum, ChecksumType, MetadataError, RepositoryReader};
use url::Url;

#[derive(Debug, thiserror::Error)]
pub enum FetchError {
    #[error("failed to read {path}: {source}")]
    ReadFile { path: PathBuf, source: io::Error },
    #[error("invalid CA certificate {path}: {source}")]
    InvalidCaCert {
        path: PathBuf,
        source: reqwest::Error,
    },
    #[error("invalid client certificate: {0}")]
    InvalidClientCert(reqwest::Error),
    #[error("failed to build HTTP client: {0}")]
    BuildClient(reqwest::Error),
    #[error("failed to join URL with {path}: {source}")]
    UrlJoin {
        path: String,
        source: url::ParseError,
    },
    #[error("failed to create directory {path}: {source}")]
    CreateDir { path: PathBuf, source: io::Error },
    #[error("HTTP request failed for {url}: {source}")]
    Request { url: String, source: reqwest::Error },
    #[error("failed to create file {path}: {source}")]
    CreateFile { path: PathBuf, source: io::Error },
    #[error("failed to write to {path}: {source}")]
    Write { path: PathBuf, source: io::Error },
    #[error("failed to read repository metadata: {0}")]
    Metadata(MetadataError),
    #[error("unknown checksum type for {0}")]
    UnknownChecksum(String),
    #[error("failed to compute checksum for {path}: {source}")]
    ChecksumCompute {
        path: PathBuf,
        source: MetadataError,
    },
    #[error("checksum mismatch for {path}: expected {expected}, got {actual}")]
    ChecksumMismatch {
        path: String,
        expected: String,
        actual: String,
    },
}

/// Configuration for downloading RPM repository metadata.
///
/// Mirrors the download options from rpmrepo's Python `DownloadConfig`.
pub struct FetchConfig {
    pub auth_token: Option<String>,
    pub allow_env_var: bool,
    pub user_agent: Option<String>,
    pub verify_tls: bool,
    pub tls_client_cert: Option<PathBuf>,
    pub tls_client_key: Option<PathBuf>,
    pub tls_ca_cert: Option<PathBuf>,
    pub all: bool,
}

impl Default for FetchConfig {
    fn default() -> Self {
        Self::new()
    }
}

impl FetchConfig {
    pub fn new() -> Self {
        Self {
            auth_token: None,
            allow_env_var: false,
            user_agent: None,
            verify_tls: true,
            tls_client_cert: None,
            tls_client_key: None,
            tls_ca_cert: None,
            all: false,
        }
    }

    fn build_client(&self) -> Result<reqwest::blocking::Client, FetchError> {
        let mut builder = ClientBuilder::new();

        if let Some(ref ua) = self.user_agent {
            builder = builder.user_agent(ua.clone());
        }

        if !self.allow_env_var {
            builder = builder.no_proxy();
        }

        if !self.verify_tls {
            builder = builder.danger_accept_invalid_certs(true);
        }

        if let Some(ref ca_path) = self.tls_ca_cert {
            let pem = fs::read(ca_path).map_err(|e| FetchError::ReadFile {
                path: ca_path.clone(),
                source: e,
            })?;
            let cert = Certificate::from_pem(&pem).map_err(|e| FetchError::InvalidCaCert {
                path: ca_path.clone(),
                source: e,
            })?;
            builder = builder.add_root_certificate(cert);
        }

        if let Some(ref cert_path) = self.tls_client_cert {
            let mut pem = fs::read(cert_path).map_err(|e| FetchError::ReadFile {
                path: cert_path.clone(),
                source: e,
            })?;
            if let Some(ref key_path) = self.tls_client_key {
                let key = fs::read(key_path).map_err(|e| FetchError::ReadFile {
                    path: key_path.clone(),
                    source: e,
                })?;
                pem.push(b'\n');
                pem.extend_from_slice(&key);
            }
            let identity = Identity::from_pem(&pem).map_err(FetchError::InvalidClientCert)?;
            builder = builder.identity(identity);
        }

        builder.build().map_err(FetchError::BuildClient)
    }
}

/// Download RPM repository metadata from a remote URL into a local directory.
///
/// Fetches `repomd.xml` followed by the primary, filelists, and other metadata files.
/// Skips the download entirely if `repomd.xml` already exists in `target_folder`.
pub fn fetch_repodata(
    base_url: Url,
    target_folder: &Path,
    config: &FetchConfig,
) -> Result<(), FetchError> {
    let client = config.build_client()?;

    let base_url = if base_url.path().ends_with('/') {
        base_url
    } else {
        let mut url = base_url;
        url.set_path(&format!("{}/", url.path()));
        url
    };

    if target_folder.join("repodata/repomd.xml").exists() {
        println!("repomd.xml already exists");
        return Ok(());
    }

    let download = |rel_path: &str| -> Result<(), FetchError> {
        let mut url = base_url.join(rel_path).map_err(|e| FetchError::UrlJoin {
            path: rel_path.to_string(),
            source: e,
        })?;
        if let Some(ref token) = config.auth_token {
            url.set_query(Some(token));
        }
        let dest = target_folder.join(rel_path);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).map_err(|e| FetchError::CreateDir {
                path: parent.to_path_buf(),
                source: e,
            })?;
        }
        let mut resp = client
            .get(url.clone())
            .send()
            .map_err(|e| FetchError::Request {
                url: url.to_string(),
                source: e,
            })?;
        let mut file = File::create(&dest).map_err(|e| FetchError::CreateFile {
            path: dest.clone(),
            source: e,
        })?;
        io::copy(&mut resp, &mut file).map_err(|e| FetchError::Write {
            path: dest.clone(),
            source: e,
        })?;
        Ok(())
    };

    download("repodata/repomd.xml")?;

    let reader =
        RepositoryReader::new_from_directory(target_folder).map_err(FetchError::Metadata)?;
    let repomd = reader.repomd();

    let download_and_verify = |rel_path: &str, expected: &Checksum| -> Result<(), FetchError> {
        download(rel_path)?;

        let dest = target_folder.join(rel_path);
        let checksum_type = match expected {
            Checksum::Md5(_) => ChecksumType::Md5,
            Checksum::Sha1(_) => ChecksumType::Sha1,
            Checksum::Sha224(_) => ChecksumType::Sha224,
            Checksum::Sha256(_) => ChecksumType::Sha256,
            Checksum::Sha384(_) => ChecksumType::Sha384,
            Checksum::Sha512(_) => ChecksumType::Sha512,
            _ => return Err(FetchError::UnknownChecksum(rel_path.to_string())),
        };
        let actual = rpmrepo_metadata::utils::checksum_file(&dest, checksum_type).map_err(|e| {
            FetchError::ChecksumCompute {
                path: dest.clone(),
                source: e,
            }
        })?;
        if actual != *expected {
            return Err(FetchError::ChecksumMismatch {
                path: rel_path.to_string(),
                expected: format!("{:?}", expected),
                actual: format!("{:?}", actual),
            });
        }
        Ok(())
    };

    if config.all {
        for record in repomd.records() {
            let rel_path = record.location_href.to_string_lossy();
            download_and_verify(&rel_path, &record.checksum)?;
        }
    } else {
        let required = [
            repomd.get_primary_data(),
            repomd.get_filelist_data(),
            repomd.get_other_data(),
        ];
        let optional = [repomd.get_updateinfo_data(), repomd.get_comps_data()];

        for record in required.into_iter().flatten() {
            let rel_path = record.location_href.to_string_lossy();
            download_and_verify(&rel_path, &record.checksum)?;
        }
        for record in optional.into_iter().flatten() {
            let rel_path = record.location_href.to_string_lossy();
            download_and_verify(&rel_path, &record.checksum)?;
        }
    }

    Ok(())
}
