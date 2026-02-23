use serde::Serialize;

/// All errors that can occur during the installation process.
#[derive(Debug, thiserror::Error)]
pub enum InstallerError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),

    #[error("Zip extraction error: {0}")]
    Zip(#[from] zip::result::ZipError),

    #[error("{0}")]
    Custom(String),
}

// Tauri requires error types to implement Serialize for IPC transport.
impl Serialize for InstallerError {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

pub type Result<T> = std::result::Result<T, InstallerError>;
