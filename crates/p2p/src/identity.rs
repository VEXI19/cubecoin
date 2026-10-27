use std::{
    io::{self, Read},
    path::Path,
};

use cubecoin_core::secure_fs::{self, SecureFsError};
use rcgen::{KeyPair, PKCS_ED25519};
use tracing::{debug, info};

use crate::error::P2pError;

pub(crate) struct PersistedIdentity {
    pub(crate) key_pair: KeyPair,
}

pub(crate) fn load_or_create_identity(directory: &Path) -> Result<PersistedIdentity, P2pError> {
    secure_fs::ensure_private_directory(directory)
        .map_err(|error| map_secure_fs_error(directory, error))?;
    let path = directory.join("identity.pk8");
    match secure_fs::open_private_file(&path, 16 * 1024) {
        Ok(mut file) => {
            let mut bytes = Vec::new();
            file.read_to_end(&mut bytes)
                .map_err(|source| P2pError::Io {
                    action: "read node identity",
                    source,
                })?;
            let key_pair = KeyPair::try_from(bytes.as_slice())
                .map_err(|error| P2pError::Certificate(error.to_string()))?;
            if key_pair.algorithm() != &PKCS_ED25519 {
                return Err(P2pError::Certificate(
                    "node identity is not Ed25519".to_owned(),
                ));
            }
            debug!(event = "identity.loaded", path = %path.display(), "node identity loaded");
            Ok(PersistedIdentity { key_pair })
        }
        Err(SecureFsError::NotFound) => {
            let key_pair = KeyPair::generate_for(&PKCS_ED25519)
                .map_err(|error| P2pError::Certificate(error.to_string()))?;
            secure_fs::write_private_file_noclobber(&path, &key_pair.serialize_der())
                .map_err(|error| map_secure_fs_error(&path, error))?;
            info!(event = "identity.created", path = %path.display(), "node identity created");
            Ok(PersistedIdentity { key_pair })
        }
        Err(error) => Err(map_secure_fs_error(&path, error)),
    }
}

fn map_secure_fs_error(path: &Path, error: SecureFsError) -> P2pError {
    match error {
        SecureFsError::UnsafeDirectory | SecureFsError::UnsafeFile | SecureFsError::TooLarge => {
            P2pError::UnsafePath(path.to_owned())
        }
        SecureFsError::NotFound => P2pError::Io {
            action: "open node identity",
            source: io::Error::new(io::ErrorKind::NotFound, "node identity does not exist"),
        },
        SecureFsError::AlreadyExists => P2pError::Io {
            action: "create node identity",
            source: io::Error::new(io::ErrorKind::AlreadyExists, "node identity already exists"),
        },
        SecureFsError::Io { source, .. } => P2pError::Io {
            action: "access node identity",
            source,
        },
    }
}
