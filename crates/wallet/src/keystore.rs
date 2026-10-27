#![cfg(target_os = "linux")]

use std::{
    ffi::OsString,
    fs::{self, File},
    io::{Read, Write},
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
};

use argon2::{Algorithm, Argon2, Block, Params, Version};
use chacha20poly1305::{
    Tag, XChaCha20Poly1305, XNonce,
    aead::{AeadInOut, KeyInit},
};
use thiserror::Error;
use zeroize::Zeroizing;

use crate::identity::WalletIdentity;
use cubecoin_core::secure_fs::{self, SecureFsError};

const MAGIC: &[u8; 8] = b"CUBEWALT";
const FORMAT_VERSION: u8 = 1;
const KDF_ALGORITHM: u8 = 1;
const AEAD_ALGORITHM: u8 = 1;
const FLAGS: u8 = 0;
const MEMORY_COST_KIB: u32 = 65_536;
const TIME_COST: u32 = 3;
const PARALLELISM: u32 = 1;
const SALT_LENGTH: usize = 32;
const NONCE_LENGTH: usize = 24;
const SEED_LENGTH: usize = 32;
const TAG_LENGTH: usize = 16;
const SEALED_LENGTH: usize = SEED_LENGTH + TAG_LENGTH;
const HEADER_LENGTH: usize = 84;
const FILE_LENGTH: usize = HEADER_LENGTH + SEALED_LENGTH;
const DEV_PATH_ENV: &str = "CUBECOIN_WALLET_DEV_PATH";
const MAX_PASSWORD_LENGTH: usize = 1024;

#[derive(Debug, Error)]
pub enum KeystoreError {
    #[error("cannot resolve the wallet identity path")]
    PathUnavailable,
    #[error("the development wallet path must be absolute")]
    RelativeDevelopmentPath,
    #[error("the development wallet path override is available only in debug builds")]
    DevelopmentPathInRelease,
    #[error("wallet password confirmation does not match")]
    PasswordMismatch,
    #[error("wallet password exceeds {MAX_PASSWORD_LENGTH} bytes")]
    PasswordTooLong,
    #[error("failed to read wallet password: {0}")]
    PasswordPrompt(#[source] std::io::Error),
    #[error("operating system entropy is unavailable: {0}")]
    Entropy(#[source] getrandom::Error),
    #[error("wallet file already exists")]
    AlreadyExists,
    #[error("wallet file is not a regular private file owned by the current user")]
    UnsafeFile,
    #[error("wallet identity directory is not a private directory owned by the current user")]
    UnsafeDirectory,
    #[error("wallet file has an invalid length")]
    InvalidLength,
    #[error("wallet file has an invalid magic value")]
    InvalidMagic,
    #[error("wallet file format version is unsupported")]
    UnsupportedVersion,
    #[error("wallet file uses an unsupported algorithm or flags")]
    UnsupportedAlgorithm,
    #[error("wallet file uses unsupported key-derivation parameters")]
    UnsupportedParameters,
    #[error("wallet file has an invalid sealed payload length")]
    InvalidSealedLength,
    #[error("incorrect password or corrupted wallet file")]
    AuthenticationFailed,
    #[error("wallet key derivation failed")]
    KeyDerivation,
    #[error("failed to {operation}: {source}")]
    Io {
        operation: &'static str,
        #[source]
        source: std::io::Error,
    },
}

pub struct Keystore {
    path: PathBuf,
    development_override: bool,
    ignored_relative_xdg_path: bool,
}

pub struct OpenedIdentity {
    pub identity: WalletIdentity,
    pub created: bool,
}

impl Keystore {
    pub fn resolve_default() -> Result<Self, KeystoreError> {
        Self::resolve_with_environment(
            std::env::var_os(DEV_PATH_ENV),
            std::env::var_os("XDG_DATA_HOME"),
            std::env::var_os("HOME"),
        )
    }

    fn resolve_with_environment(
        development_path: Option<OsString>,
        xdg_data_home: Option<OsString>,
        home: Option<OsString>,
    ) -> Result<Self, KeystoreError> {
        if let Some(path) = development_path {
            if !cfg!(debug_assertions) {
                return Err(KeystoreError::DevelopmentPathInRelease);
            }

            let path = PathBuf::from(path);
            if !path.is_absolute() {
                return Err(KeystoreError::RelativeDevelopmentPath);
            }

            return Ok(Self {
                path,
                development_override: true,
                ignored_relative_xdg_path: false,
            });
        }

        let has_xdg_data_home = xdg_data_home.is_some();
        let xdg_path = xdg_data_home
            .map(PathBuf::from)
            .filter(|path| !path.as_os_str().is_empty() && path.is_absolute());
        let ignored_relative_xdg_path = has_xdg_data_home && xdg_path.is_none();

        let data_home = match xdg_path {
            Some(path) => path,
            None => {
                let home = PathBuf::from(home.ok_or(KeystoreError::PathUnavailable)?);
                if home.as_os_str().is_empty() || !home.is_absolute() {
                    return Err(KeystoreError::PathUnavailable);
                }
                home.join(".local/share")
            }
        };

        Ok(Self {
            path: data_home.join("cubecoin/identity/wallet.bin"),
            development_override: false,
            ignored_relative_xdg_path,
        })
    }

    pub fn uses_development_override(&self) -> bool {
        self.development_override
    }

    pub fn ignored_relative_xdg_path(&self) -> bool {
        self.ignored_relative_xdg_path
    }

    pub fn load_or_create(&self) -> Result<OpenedIdentity, KeystoreError> {
        self.ensure_parent_directory()?;

        match fs::symlink_metadata(&self.path) {
            Ok(_) => self.load(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => self.create(),
            Err(error) => Err(io_error("inspect wallet file", error)),
        }
    }

    fn create(&self) -> Result<OpenedIdentity, KeystoreError> {
        write_stderr(
            "A new wallet identity will be created. Back up the encrypted wallet file and store its password separately; there is no recovery if either is lost.",
        );
        let password = prompt_new_password()?;

        if password.is_empty() {
            write_stderr(
                "warning: an empty wallet password provides no protection if the wallet file is stolen",
            );
        }

        self.create_with_password(password.as_bytes())
    }

    fn create_with_password(&self, password: &[u8]) -> Result<OpenedIdentity, KeystoreError> {
        self.ensure_parent_directory()?;
        let identity = WalletIdentity::generate().map_err(KeystoreError::Entropy)?;
        let seed = identity.seed();
        let file = WalletFile::seal(password, &seed)?;

        self.write_new(&file)?;

        Ok(OpenedIdentity {
            identity,
            created: true,
        })
    }

    fn load(&self) -> Result<OpenedIdentity, KeystoreError> {
        let password = prompt_password("Wallet password: ")?;
        self.load_with_password(password.as_bytes())
    }

    fn load_with_password(&self, password: &[u8]) -> Result<OpenedIdentity, KeystoreError> {
        self.ensure_parent_directory()?;
        let file = self.read_file()?;
        let seed = file.open(password)?;

        Ok(OpenedIdentity {
            identity: WalletIdentity::from_seed(&seed),
            created: false,
        })
    }

    fn ensure_parent_directory(&self) -> Result<(), KeystoreError> {
        let parent = self.path.parent().ok_or(KeystoreError::PathUnavailable)?;

        if self.development_override {
            ensure_private_directory(parent)?;
            return Ok(());
        }

        let cubecoin = parent.parent().ok_or(KeystoreError::PathUnavailable)?;
        let data_home = cubecoin.parent().ok_or(KeystoreError::PathUnavailable)?;
        fs::create_dir_all(data_home)
            .map_err(|error| io_error("create wallet data directory", error))?;
        ensure_private_directory(cubecoin)?;
        ensure_private_directory(parent)
    }

    fn write_new(&self, file: &WalletFile) -> Result<(), KeystoreError> {
        secure_fs::write_private_file_noclobber(&self.path, &file.bytes)
            .map_err(map_secure_fs_error)
    }

    fn read_file(&self) -> Result<WalletFile, KeystoreError> {
        let mut file = open_private_file(&self.path)?;
        let metadata = file
            .metadata()
            .map_err(|error| io_error("read wallet metadata", error))?;
        validate_private_file(&metadata)?;

        if metadata.len() != FILE_LENGTH as u64 {
            return Err(KeystoreError::InvalidLength);
        }

        let mut bytes = [0_u8; FILE_LENGTH];
        file.read_exact(&mut bytes)
            .map_err(|error| io_error("read wallet file", error))?;

        WalletFile::parse(bytes)
    }
}

struct WalletFile {
    bytes: [u8; FILE_LENGTH],
}

impl WalletFile {
    fn seal(password: &[u8], seed: &[u8; SEED_LENGTH]) -> Result<Self, KeystoreError> {
        let mut salt = [0_u8; SALT_LENGTH];
        let mut nonce = [0_u8; NONCE_LENGTH];
        getrandom::fill(&mut salt).map_err(KeystoreError::Entropy)?;
        getrandom::fill(&mut nonce).map_err(KeystoreError::Entropy)?;

        let mut bytes = [0_u8; FILE_LENGTH];
        bytes[..8].copy_from_slice(MAGIC);
        bytes[8] = FORMAT_VERSION;
        bytes[9] = KDF_ALGORITHM;
        bytes[10] = AEAD_ALGORITHM;
        bytes[11] = FLAGS;
        bytes[12..16].copy_from_slice(&MEMORY_COST_KIB.to_le_bytes());
        bytes[16..20].copy_from_slice(&TIME_COST.to_le_bytes());
        bytes[20..24].copy_from_slice(&PARALLELISM.to_le_bytes());
        bytes[24..56].copy_from_slice(&salt);
        bytes[56..80].copy_from_slice(&nonce);
        bytes[80..84].copy_from_slice(&(SEALED_LENGTH as u32).to_le_bytes());

        let key = derive_key(password, &salt)?;
        let cipher = XChaCha20Poly1305::new_from_slice(&key[..])
            .map_err(|_| KeystoreError::KeyDerivation)?;
        let mut ciphertext = Zeroizing::new(seed.to_vec());
        let nonce = XNonce::try_from(nonce.as_slice()).map_err(|_| KeystoreError::KeyDerivation)?;
        let tag = cipher
            .encrypt_inout_detached(
                &nonce,
                &bytes[..HEADER_LENGTH],
                (&mut ciphertext[..]).into(),
            )
            .map_err(|_| KeystoreError::AuthenticationFailed)?;

        bytes[HEADER_LENGTH..HEADER_LENGTH + SEED_LENGTH].copy_from_slice(&ciphertext);
        bytes[HEADER_LENGTH + SEED_LENGTH..].copy_from_slice(&tag);

        Ok(Self { bytes })
    }

    fn parse(bytes: [u8; FILE_LENGTH]) -> Result<Self, KeystoreError> {
        if &bytes[..8] != MAGIC {
            return Err(KeystoreError::InvalidMagic);
        }
        if bytes[8] != FORMAT_VERSION {
            return Err(KeystoreError::UnsupportedVersion);
        }
        if bytes[9] != KDF_ALGORITHM || bytes[10] != AEAD_ALGORITHM || bytes[11] != FLAGS {
            return Err(KeystoreError::UnsupportedAlgorithm);
        }
        if u32::from_le_bytes(bytes[12..16].try_into().expect("fixed slice length"))
            != MEMORY_COST_KIB
            || u32::from_le_bytes(bytes[16..20].try_into().expect("fixed slice length"))
                != TIME_COST
            || u32::from_le_bytes(bytes[20..24].try_into().expect("fixed slice length"))
                != PARALLELISM
        {
            return Err(KeystoreError::UnsupportedParameters);
        }
        if u32::from_le_bytes(bytes[80..84].try_into().expect("fixed slice length"))
            != SEALED_LENGTH as u32
        {
            return Err(KeystoreError::InvalidSealedLength);
        }

        Ok(Self { bytes })
    }

    fn open(&self, password: &[u8]) -> Result<Zeroizing<[u8; SEED_LENGTH]>, KeystoreError> {
        let salt: [u8; SALT_LENGTH] = self.bytes[24..56].try_into().expect("fixed slice length");
        let nonce: [u8; NONCE_LENGTH] = self.bytes[56..80].try_into().expect("fixed slice length");
        let tag: Tag = self.bytes[HEADER_LENGTH + SEED_LENGTH..]
            .try_into()
            .expect("fixed slice length");
        let key = derive_key(password, &salt)?;
        let cipher = XChaCha20Poly1305::new_from_slice(&key[..])
            .map_err(|_| KeystoreError::KeyDerivation)?;
        let mut seed = Zeroizing::new([0_u8; SEED_LENGTH]);
        seed.copy_from_slice(&self.bytes[HEADER_LENGTH..HEADER_LENGTH + SEED_LENGTH]);
        let nonce = XNonce::try_from(nonce.as_slice()).map_err(|_| KeystoreError::KeyDerivation)?;

        cipher
            .decrypt_inout_detached(
                &nonce,
                &self.bytes[..HEADER_LENGTH],
                (&mut seed[..]).into(),
                &tag,
            )
            .map_err(|_| KeystoreError::AuthenticationFailed)?;

        Ok(seed)
    }
}

fn derive_key(
    password: &[u8],
    salt: &[u8; SALT_LENGTH],
) -> Result<Zeroizing<[u8; 32]>, KeystoreError> {
    let params = Params::new(MEMORY_COST_KIB, TIME_COST, PARALLELISM, Some(32))
        .map_err(|_| KeystoreError::KeyDerivation)?;
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params.clone());
    let mut key = Zeroizing::new([0_u8; 32]);
    let mut blocks = Zeroizing::new(Vec::<Block>::new());
    blocks
        .try_reserve_exact(params.block_count())
        .map_err(|_| KeystoreError::KeyDerivation)?;
    blocks.resize(params.block_count(), Block::new());

    argon2
        .hash_password_into_with_memory(password, salt, &mut *key, &mut *blocks)
        .map_err(|_| KeystoreError::KeyDerivation)?;

    Ok(key)
}

fn prompt_new_password() -> Result<Zeroizing<String>, KeystoreError> {
    let password = prompt_password("Create wallet password: ")?;
    let confirmation = prompt_password("Confirm wallet password: ")?;

    if password.as_bytes() != confirmation.as_bytes() {
        return Err(KeystoreError::PasswordMismatch);
    }

    Ok(password)
}

fn prompt_password(message: &str) -> Result<Zeroizing<String>, KeystoreError> {
    let password = rpassword::prompt_password(message).map_err(KeystoreError::PasswordPrompt)?;
    if password.len() > MAX_PASSWORD_LENGTH {
        return Err(KeystoreError::PasswordTooLong);
    }

    Ok(Zeroizing::new(password))
}

fn ensure_private_directory(path: &Path) -> Result<(), KeystoreError> {
    secure_fs::ensure_private_directory(path).map_err(map_secure_fs_error)
}

#[cfg(test)]
fn validate_private_directory(metadata: &fs::Metadata) -> Result<(), KeystoreError> {
    if metadata.file_type().is_symlink()
        || !metadata.is_dir()
        || metadata.uid() != current_uid()
        || metadata.permissions().mode() & 0o777 != 0o700
    {
        return Err(KeystoreError::UnsafeDirectory);
    }

    Ok(())
}

fn open_private_file(path: &Path) -> Result<File, KeystoreError> {
    secure_fs::open_private_file(path, FILE_LENGTH as u64).map_err(map_secure_fs_error)
}

fn validate_private_file(metadata: &fs::Metadata) -> Result<(), KeystoreError> {
    if !metadata.file_type().is_file()
        || metadata.uid() != current_uid()
        || metadata.permissions().mode() & 0o777 != 0o600
        || metadata.nlink() != 1
    {
        return Err(KeystoreError::UnsafeFile);
    }

    Ok(())
}

fn current_uid() -> u32 {
    // SAFETY: `geteuid` takes no pointers and has no Rust memory-safety preconditions.
    unsafe { libc::geteuid() }
}

fn map_secure_fs_error(error: SecureFsError) -> KeystoreError {
    match error {
        SecureFsError::AlreadyExists => KeystoreError::AlreadyExists,
        SecureFsError::UnsafeDirectory => KeystoreError::UnsafeDirectory,
        SecureFsError::UnsafeFile => KeystoreError::UnsafeFile,
        SecureFsError::TooLarge => KeystoreError::InvalidLength,
        SecureFsError::NotFound => io_error(
            "open wallet file",
            std::io::Error::new(std::io::ErrorKind::NotFound, "wallet file does not exist"),
        ),
        SecureFsError::Io { source, .. } => io_error("access wallet file", source),
    }
}

fn io_error(operation: &'static str, source: std::io::Error) -> KeystoreError {
    KeystoreError::Io { operation, source }
}

fn write_stderr(message: &str) {
    let _ = writeln!(std::io::stderr(), "{message}");
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::{
        AEAD_ALGORITHM, FILE_LENGTH, FLAGS, FORMAT_VERSION, HEADER_LENGTH, KDF_ALGORITHM, Keystore,
        KeystoreError, MAGIC, MEMORY_COST_KIB, NONCE_LENGTH, PARALLELISM, SALT_LENGTH,
        SEALED_LENGTH, TAG_LENGTH, TIME_COST, WalletFile,
    };

    fn valid_header() -> [u8; FILE_LENGTH] {
        let mut bytes = [0_u8; FILE_LENGTH];
        bytes[..8].copy_from_slice(MAGIC);
        bytes[8] = FORMAT_VERSION;
        bytes[9] = KDF_ALGORITHM;
        bytes[10] = AEAD_ALGORITHM;
        bytes[11] = FLAGS;
        bytes[12..16].copy_from_slice(&MEMORY_COST_KIB.to_le_bytes());
        bytes[16..20].copy_from_slice(&TIME_COST.to_le_bytes());
        bytes[20..24].copy_from_slice(&PARALLELISM.to_le_bytes());
        bytes[80..84].copy_from_slice(&(SEALED_LENGTH as u32).to_le_bytes());
        bytes
    }

    #[test]
    fn encrypts_and_decrypts_a_seed() {
        let seed = [0x42; 32];
        let file = WalletFile::seal(b"test password", &seed).expect("encrypt seed");

        assert_eq!(*file.open(b"test password").expect("decrypt seed"), seed);
    }

    #[test]
    fn supports_an_empty_password() {
        let seed = [0x24; 32];
        let file = WalletFile::seal(b"", &seed).expect("encrypt seed");

        assert_eq!(*file.open(b"").expect("decrypt seed"), seed);
    }

    #[test]
    fn rejects_a_wrong_password_and_tampered_header() {
        let mut file = WalletFile::seal(b"test password", &[0x42; 32]).expect("encrypt seed");
        assert!(matches!(
            file.open(b"wrong password"),
            Err(KeystoreError::AuthenticationFailed)
        ));

        file.bytes[12] ^= 1;
        assert!(matches!(
            WalletFile::parse(file.bytes),
            Err(KeystoreError::UnsupportedParameters)
        ));
    }

    #[test]
    fn rejects_tampered_ciphertext() {
        let mut file = WalletFile::seal(b"test password", &[0x42; 32]).expect("encrypt seed");
        file.bytes[84] ^= 1;

        assert!(matches!(
            file.open(b"test password"),
            Err(KeystoreError::AuthenticationFailed)
        ));
    }

    #[test]
    fn rejects_tampered_salt_and_nonce() {
        let file = WalletFile::seal(b"test password", &[0x42; 32]).expect("encrypt seed");

        for offset in [24, 56] {
            let mut tampered = file.bytes;
            tampered[offset] ^= 1;
            let tampered = WalletFile::parse(tampered).expect("header remains structurally valid");
            assert!(matches!(
                tampered.open(b"test password"),
                Err(KeystoreError::AuthenticationFailed)
            ));
        }
    }

    #[test]
    fn validates_the_fixed_v1_header() {
        assert_eq!(FILE_LENGTH, 132);
        assert_eq!(HEADER_LENGTH, 84);
        assert_eq!(SEALED_LENGTH, 48);
        assert_eq!(SALT_LENGTH, 32);
        assert_eq!(NONCE_LENGTH, 24);
        assert_eq!(TAG_LENGTH, 16);
        assert!(WalletFile::parse(valid_header()).is_ok());
    }

    #[test]
    fn rejects_unknown_header_variants() {
        for (offset, expected) in [
            (0, KeystoreError::InvalidMagic),
            (8, KeystoreError::UnsupportedVersion),
            (9, KeystoreError::UnsupportedAlgorithm),
            (10, KeystoreError::UnsupportedAlgorithm),
            (11, KeystoreError::UnsupportedAlgorithm),
            (16, KeystoreError::UnsupportedParameters),
            (20, KeystoreError::UnsupportedParameters),
            (80, KeystoreError::InvalidSealedLength),
        ] {
            let mut bytes = valid_header();
            bytes[offset] ^= 1;
            let error = match WalletFile::parse(bytes) {
                Ok(_) => panic!("header must be rejected"),
                Err(error) => error,
            };
            assert_eq!(error.to_string(), expected.to_string());
        }
    }

    #[test]
    fn requires_private_wallet_files() {
        let directory = tempfile::tempdir().expect("create temporary directory");
        let path = directory.path().join("wallet.bin");
        std::fs::write(&path, [0_u8; FILE_LENGTH]).expect("write test wallet");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644))
            .expect("set test permissions");
        let metadata = std::fs::metadata(path).expect("read test metadata");

        assert!(super::validate_private_file(&metadata).is_err());
    }

    #[test]
    fn creates_a_private_wallet_without_overwriting_it() {
        let directory = tempfile::tempdir().expect("create temporary directory");
        let keystore = Keystore {
            path: directory.path().join("identity/wallet.bin"),
            development_override: true,
            ignored_relative_xdg_path: false,
        };

        let created = keystore
            .create_with_password(b"test password")
            .expect("create wallet");
        let metadata = std::fs::metadata(&keystore.path).expect("read wallet metadata");
        let parent = std::fs::metadata(keystore.path.parent().expect("wallet parent"))
            .expect("read wallet directory metadata");
        assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
        assert_eq!(parent.permissions().mode() & 0o777, 0o700);
        assert_eq!(metadata.len(), FILE_LENGTH as u64);

        let loaded = keystore
            .load_with_password(b"test password")
            .expect("load wallet");
        assert_eq!(created.identity.address(), loaded.identity.address());
        assert!(matches!(
            keystore.create_with_password(b"another password"),
            Err(KeystoreError::AlreadyExists)
        ));
    }

    #[test]
    fn rejects_symlinked_and_hard_linked_wallet_files() {
        let directory = tempfile::tempdir().expect("create temporary directory");
        let target = directory.path().join("target.bin");
        std::fs::write(&target, [0_u8; FILE_LENGTH]).expect("write target");
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o600))
            .expect("set target permissions");

        let hard_link = directory.path().join("hard-link.bin");
        std::fs::hard_link(&target, &hard_link).expect("create hard link");
        let metadata = std::fs::metadata(&hard_link).expect("read hard-link metadata");
        assert!(super::validate_private_file(&metadata).is_err());

        let symlink = directory.path().join("symlink.bin");
        std::os::unix::fs::symlink(&target, &symlink).expect("create symlink");
        assert!(super::open_private_file(&symlink).is_err());
    }

    #[test]
    fn rejects_unsafe_directories_and_invalid_file_lengths() {
        let directory = tempfile::tempdir().expect("create temporary directory");
        let unsafe_directory = directory.path().join("unsafe");
        std::fs::create_dir(&unsafe_directory).expect("create unsafe directory");
        std::fs::set_permissions(&unsafe_directory, std::fs::Permissions::from_mode(0o755))
            .expect("set unsafe directory permissions");
        let metadata =
            std::fs::metadata(&unsafe_directory).expect("read unsafe directory metadata");
        assert!(super::validate_private_directory(&metadata).is_err());

        let path = directory.path().join("identity/wallet.bin");
        let keystore = Keystore {
            path: path.clone(),
            development_override: true,
            ignored_relative_xdg_path: false,
        };
        keystore
            .ensure_parent_directory()
            .expect("create identity directory");
        std::fs::write(&path, [0_u8; FILE_LENGTH - 1]).expect("write short wallet");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
            .expect("set short wallet permissions");
        assert!(matches!(
            keystore.read_file(),
            Err(KeystoreError::InvalidLength)
        ));
    }

    #[test]
    fn resolves_only_absolute_development_and_data_paths() {
        #[cfg(debug_assertions)]
        {
            let development = Keystore::resolve_with_environment(
                Some("/tmp/cubecoin-wallet.bin".into()),
                None,
                None,
            )
            .expect("resolve absolute development path");
            assert!(development.uses_development_override());

            assert!(matches!(
                Keystore::resolve_with_environment(Some("relative.bin".into()), None, None),
                Err(KeystoreError::RelativeDevelopmentPath)
            ));
        }

        #[cfg(not(debug_assertions))]
        assert!(matches!(
            Keystore::resolve_with_environment(Some("/tmp/cubecoin-wallet.bin".into()), None, None,),
            Err(KeystoreError::DevelopmentPathInRelease)
        ));

        let fallback = Keystore::resolve_with_environment(
            None,
            Some("relative-xdg".into()),
            Some("/home/tester".into()),
        )
        .expect("fall back from relative XDG path");
        assert!(fallback.ignored_relative_xdg_path());

        assert!(matches!(
            Keystore::resolve_with_environment(None, None, Some("relative-home".into())),
            Err(KeystoreError::PathUnavailable)
        ));
    }
}
