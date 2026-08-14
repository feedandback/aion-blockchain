use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use argon2::Argon2;
use chacha20poly1305::{
    ChaCha20Poly1305, Nonce,
    aead::{Aead, Generate, Key, KeyInit},
};

use crate::core::Block;

const DEFAULT_DATA_DIRECTORY: &str = "data";

const DATA_DIRECTORY_ENV: &str = "KYBERNETES_DATA_DIR";

const LEGACY_DATA_DIRECTORY_ENV: &str = "AION_DATA_DIR";

const BLOCKCHAIN_FILE_NAME: &str = "blockchain.json";

const BLOCKCHAIN_TEMP_FILE_NAME: &str = "blockchain.tmp";

const WALLETS_FILE_NAME: &str = "wallets.json";

const WALLETS_TEMP_FILE_NAME: &str = "wallets.tmp";

#[derive(serde::Serialize, serde::Deserialize)]
struct StoredWallets {
    alice_private_key: String,
    bob_private_key: String,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct EncryptedStoredWallets {
    version: u32,
    kdf: String,
    cipher: String,
    salt_hex: String,
    nonce_hex: String,
    ciphertext_hex: String,
}

pub struct Storage;

#[allow(dead_code)]
impl Storage {
    pub fn data_directory() -> PathBuf {
        match std::env::var(DATA_DIRECTORY_ENV)
            .or_else(|_| std::env::var(LEGACY_DATA_DIRECTORY_ENV))
        {
            Ok(value) if !value.trim().is_empty() => PathBuf::from(value),

            _ => PathBuf::from(DEFAULT_DATA_DIRECTORY),
        }
    }

    pub fn blockchain_path() -> PathBuf {
        Self::blockchain_path_in(&Self::data_directory())
    }

    fn blockchain_temp_path() -> PathBuf {
        Self::blockchain_temp_path_in(&Self::data_directory())
    }

    fn blockchain_path_in(data_directory: &Path) -> PathBuf {
        data_directory.join(BLOCKCHAIN_FILE_NAME)
    }

    fn blockchain_temp_path_in(data_directory: &Path) -> PathBuf {
        data_directory.join(BLOCKCHAIN_TEMP_FILE_NAME)
    }

    pub fn wallets_path() -> PathBuf {
        Self::data_directory().join(WALLETS_FILE_NAME)
    }

    fn wallets_temp_path() -> PathBuf {
        Self::data_directory().join(WALLETS_TEMP_FILE_NAME)
    }

    fn ensure_data_directory() -> Result<(), String> {
        let data_directory = Self::data_directory();

        Self::ensure_data_directory_at(&data_directory)
    }

    fn unique_blockchain_temp_path(data_directory: &Path) -> PathBuf {
        let random_name = hex::encode(chacha20poly1305::Key::generate());
        data_directory.join(format!(".{BLOCKCHAIN_TEMP_FILE_NAME}-{random_name}"))
    }

    fn unique_wallet_temp_path(data_directory: &Path) -> PathBuf {
        let random_name = hex::encode(chacha20poly1305::Key::generate());
        data_directory.join(format!(".{WALLETS_TEMP_FILE_NAME}-{random_name}"))
    }

    fn ensure_data_directory_at(data_directory: &Path) -> Result<(), String> {
        fs::create_dir_all(data_directory).map_err(|error| {
            format!(
                "Data directory could not be created ({}): {}",
                data_directory.display(),
                error
            )
        })
    }

    pub fn save_blockchain(chain: &[Block]) -> Result<(), String> {
        let data_directory = Self::data_directory();

        Self::save_blockchain_to(&data_directory, chain)
    }

    pub fn save_blockchain_to(data_directory: &Path, chain: &[Block]) -> Result<(), String> {
        if chain.is_empty() {
            return Err("Empty blockchain cannot be saved".into());
        }

        Self::ensure_data_directory_at(data_directory)?;

        let serialized = serde_json::to_vec_pretty(chain)
            .map_err(|error| format!("Blockchain could not be serialized to JSON: {}", error))?;

        let temp_path = Self::unique_blockchain_temp_path(data_directory);

        let final_path = Self::blockchain_path_in(data_directory);

        {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp_path)
                .map_err(|error| {
                    format!("Temporary blockchain file could not be created: {}", error)
                })?;

            file.write_all(&serialized)
                .map_err(|error| format!("Blockchain could not be written to disk: {}", error))?;

            file.sync_all().map_err(|error| {
                format!(
                    "Blockchain file could not be synchronized to disk: {}",
                    error
                )
            })?;
        }

        Self::replace_file(&temp_path, &final_path).map_err(|error| {
            format!(
                "Blockchain file could not be moved into the active location: {}",
                error
            )
        })?;

        #[cfg(unix)]
        if let Err(error) = File::open(data_directory).and_then(|directory| directory.sync_all()) {
            eprintln!(
                "Blockchain file was replaced atomically, but data directory fsync failed: {error}"
            );
        }

        Ok(())
    }

    #[cfg(not(windows))]
    fn replace_file(temp_path: &Path, final_path: &Path) -> std::io::Result<()> {
        fs::rename(temp_path, final_path)
    }

    #[cfg(windows)]
    fn replace_file(temp_path: &Path, final_path: &Path) -> std::io::Result<()> {
        use std::iter;
        use std::os::windows::ffi::OsStrExt;

        const MOVEFILE_REPLACE_EXISTING: u32 = 0x1;
        const MOVEFILE_WRITE_THROUGH: u32 = 0x8;

        #[link(name = "Kernel32")]
        unsafe extern "system" {
            fn MoveFileExW(
                existing_file_name: *const u16,
                new_file_name: *const u16,
                flags: u32,
            ) -> i32;
        }

        let existing_file_name = temp_path
            .as_os_str()
            .encode_wide()
            .chain(iter::once(0))
            .collect::<Vec<_>>();
        let new_file_name = final_path
            .as_os_str()
            .encode_wide()
            .chain(iter::once(0))
            .collect::<Vec<_>>();
        let replaced = unsafe {
            MoveFileExW(
                existing_file_name.as_ptr(),
                new_file_name.as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        };

        if replaced == 0 {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    pub fn load_blockchain() -> Result<Option<Vec<Block>>, String> {
        let data_directory = Self::data_directory();

        Self::load_blockchain_from(&data_directory)
    }

    pub fn load_blockchain_from(data_directory: &Path) -> Result<Option<Vec<Block>>, String> {
        let path = Self::blockchain_path_in(data_directory);

        let mut file = match File::open(&path) {
            Ok(file) => file,

            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(None);
            }

            Err(error) => {
                return Err(format!("Blockchain file could not be opened: {}", error));
            }
        };

        let mut bytes = Vec::new();

        file.read_to_end(&mut bytes)
            .map_err(|error| format!("Blockchain file could not be read: {}", error))?;

        if bytes.is_empty() {
            return Err("Blockchain file is empty".into());
        }

        let chain: Vec<Block> = serde_json::from_slice(&bytes)
            .map_err(|error| format!("Blockchain file contains invalid JSON: {}", error))?;

        if chain.is_empty() {
            return Err("Stored blockchain is empty".into());
        }

        Ok(Some(chain))
    }

    fn derive_wallet_key(password: &str, salt: &[u8]) -> Result<[u8; 32], String> {
        if password.len() < 12 {
            return Err("Wallet password must be at least 12 characters long".into());
        }

        let mut key = [0u8; 32];

        Argon2::default()
            .hash_password_into(password.as_bytes(), salt, &mut key)
            .map_err(|error| format!("Wallet encryption key could not be derived: {}", error))?;

        Ok(key)
    }

    pub fn save_wallet_private_keys(
        password: &str,
        alice_private_key: &str,
        bob_private_key: &str,
    ) -> Result<(), String> {
        let data_directory = Self::data_directory();

        Self::ensure_data_directory_at(&data_directory)?;

        let stored_wallets = StoredWallets {
            alice_private_key: alice_private_key.to_string(),
            bob_private_key: bob_private_key.to_string(),
        };

        let plaintext = serde_json::to_vec(&stored_wallets)
            .map_err(|error| format!("Wallet data could not be serialized to JSON: {}", error))?;

        let random_salt_material = Key::<ChaCha20Poly1305>::generate();

        let mut salt = [0u8; 16];

        salt.copy_from_slice(&random_salt_material.as_slice()[..16]);

        let key = Self::derive_wallet_key(password, &salt)?;

        let cipher = ChaCha20Poly1305::new_from_slice(&key)
            .map_err(|_| "Wallet encryption key is invalid".to_string())?;

        let nonce = Nonce::generate();

        let ciphertext = cipher
            .encrypt(&nonce, plaintext.as_ref())
            .map_err(|_| "Wallet private keys could not be encrypted".to_string())?;

        let encrypted_wallets = EncryptedStoredWallets {
            version: 1,
            kdf: "argon2id".to_string(),
            cipher: "chacha20poly1305".to_string(),
            salt_hex: hex::encode(salt),
            nonce_hex: hex::encode(nonce.as_slice()),
            ciphertext_hex: hex::encode(ciphertext),
        };

        let serialized = serde_json::to_vec_pretty(&encrypted_wallets).map_err(|error| {
            format!(
                "Encrypted wallet data could not be serialized to JSON: {}",
                error
            )
        })?;

        let temp_path = Self::unique_wallet_temp_path(&data_directory);

        let final_path = data_directory.join(WALLETS_FILE_NAME);

        {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp_path)
                .map_err(|error| {
                    format!("Temporary wallet file could not be created: {}", error)
                })?;

            file.write_all(&serialized).map_err(|error| {
                format!(
                    "Encrypted wallet data could not be written to disk: {}",
                    error
                )
            })?;

            file.sync_all().map_err(|error| {
                format!("Wallet file could not be synchronized to disk: {}", error)
            })?;
        }

        Self::replace_file(&temp_path, &final_path).map_err(|error| {
            format!(
                "Wallet file could not be moved into the active location: {}",
                error
            )
        })?;

        #[cfg(unix)]
        if let Err(error) = File::open(&data_directory).and_then(|directory| directory.sync_all()) {
            eprintln!(
                "Wallet file was replaced atomically, but data directory fsync failed: {error}"
            );
        }

        Ok(())
    }

    pub fn load_wallet_private_keys(password: &str) -> Result<Option<(String, String)>, String> {
        let path = Self::wallets_path();

        if !path.exists() {
            return Ok(None);
        }

        let mut file = File::open(&path)
            .map_err(|error| format!("Wallet file could not be opened: {}", error))?;

        let mut bytes = Vec::new();

        file.read_to_end(&mut bytes)
            .map_err(|error| format!("Wallet file could not be read: {}", error))?;

        if bytes.is_empty() {
            return Err("Wallet file is empty".into());
        }

        if let Ok(encrypted_wallets) = serde_json::from_slice::<EncryptedStoredWallets>(&bytes) {
            if encrypted_wallets.version != 1 {
                return Err("Wallet file version is not supported".into());
            }

            if encrypted_wallets.kdf != "argon2id" || encrypted_wallets.cipher != "chacha20poly1305"
            {
                return Err("Wallet encryption format is not supported".into());
            }

            let salt = hex::decode(&encrypted_wallets.salt_hex)
                .map_err(|_| "Wallet salt format is invalid".to_string())?;

            if salt.len() != 16 {
                return Err("Wallet salt length is invalid".into());
            }

            let nonce_bytes = hex::decode(&encrypted_wallets.nonce_hex)
                .map_err(|_| "Wallet nonce format is invalid".to_string())?;

            let nonce_array: [u8; 12] = nonce_bytes
                .try_into()
                .map_err(|_| "Wallet nonce length is invalid".to_string())?;

            let ciphertext = hex::decode(&encrypted_wallets.ciphertext_hex)
                .map_err(|_| "Wallet ciphertext format is invalid".to_string())?;

            let key = Self::derive_wallet_key(password, &salt)?;

            let cipher = ChaCha20Poly1305::new_from_slice(&key)
                .map_err(|_| "Wallet decryption key is invalid".to_string())?;

            let nonce = Nonce::from(nonce_array);

            let plaintext = cipher.decrypt(&nonce, ciphertext.as_ref()).map_err(|_| {
                "Wallet password is incorrect or the wallet file is corrupted".to_string()
            })?;

            let stored_wallets: StoredWallets = serde_json::from_slice(&plaintext)
                .map_err(|error| format!("Decrypted wallet data is invalid: {}", error))?;

            return Ok(Some((
                stored_wallets.alice_private_key,
                stored_wallets.bob_private_key,
            )));
        }

        // Migrate the legacy plaintext wallets.json file once.
        let legacy_wallets: StoredWallets = serde_json::from_slice(&bytes)
            .map_err(|_| "Wallet file is invalid or uses an unsupported format".to_string())?;

        Self::save_wallet_private_keys(
            password,
            &legacy_wallets.alice_private_key,
            &legacy_wallets.bob_private_key,
        )?;

        println!("Legacy plaintext wallet file was migrated to the encrypted format.");

        Ok(Some((
            legacy_wallets.alice_private_key,
            legacy_wallets.bob_private_key,
        )))
    }

    pub fn blockchain_exists() -> bool {
        Self::blockchain_path().exists()
    }

    pub fn delete_blockchain() -> Result<(), String> {
        let path = Self::blockchain_path();

        if !path.exists() {
            return Ok(());
        }

        fs::remove_file(path)
            .map_err(|error| format!("Blockchain file could not be deleted: {}", error))
    }
}
