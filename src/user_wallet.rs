use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::{
    ChaCha20Poly1305, KeyInit, Nonce,
    aead::{Aead, Generate, Key, Payload},
};

use crate::wallet::Wallet;

pub const USER_WALLET_FILE_NAME: &str = "user-wallet.json";

const USER_WALLET_FORMAT: &str = "kybernetes-user-wallet-v1";
const USER_WALLET_AAD: &[u8] = b"KYBERNETES_USER_WALLET_V1";

const ARGON2_MEMORY_KIB: u32 = 19_456;
const ARGON2_ITERATIONS: u32 = 2;
const ARGON2_PARALLELISM: u32 = 1;

#[derive(serde::Serialize, serde::Deserialize)]
struct StoredUserWallet {
    format: String,
    address: String,
    private_key_hex: String,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct EncryptedUserWallet {
    version: u32,
    kdf: String,
    kdf_memory_kib: u32,
    kdf_iterations: u32,
    kdf_parallelism: u32,
    cipher: String,
    salt_hex: String,
    nonce_hex: String,
    ciphertext_hex: String,
}

pub struct UserWalletKeystore {
    data_directory: PathBuf,
}

impl UserWalletKeystore {
    pub fn at(data_directory: impl Into<PathBuf>) -> Self {
        Self {
            data_directory: data_directory.into(),
        }
    }

    pub fn path(&self) -> PathBuf {
        self.data_directory.join(USER_WALLET_FILE_NAME)
    }

    pub fn exists(&self) -> bool {
        self.path().exists()
    }

    pub fn create(&self, password: &str) -> Result<Wallet, String> {
        if self.exists() {
            return Err("User wallet already exists; automatic overwrite was rejected".into());
        }

        fs::create_dir_all(&self.data_directory)
            .map_err(|error| format!("User wallet directory could not be created: {error}"))?;

        let wallet = Wallet::new();

        let stored = StoredUserWallet {
            format: USER_WALLET_FORMAT.to_string(),
            address: wallet.address().to_string(),
            private_key_hex: wallet.private_key_hex(),
        };

        let plaintext = serde_json::to_vec(&stored)
            .map_err(|error| format!("User wallet data could not be serialized: {error}"))?;

        let encrypted = Self::encrypt(password, &plaintext)?;

        self.write_new(&encrypted)?;

        Ok(wallet)
    }

    pub fn load(&self, password: &str) -> Result<Option<Wallet>, String> {
        let path = self.path();

        if !path.exists() {
            return Ok(None);
        }

        let encrypted = fs::read(&path)
            .map_err(|error| format!("User wallet file could not be read: {error}"))?;

        let plaintext = Self::decrypt(password, &encrypted)?;

        let stored: StoredUserWallet = serde_json::from_slice(&plaintext)
            .map_err(|_| "User wallet data format is invalid".to_string())?;

        if stored.format != USER_WALLET_FORMAT {
            return Err("User wallet format is not supported".into());
        }

        let wallet = Wallet::from_private_key_hex(&stored.private_key_hex)?;

        if wallet.address() != stored.address {
            return Err("User wallet address does not match the private key".into());
        }

        Ok(Some(wallet))
    }

    fn derive_key(password: &str, salt: &[u8]) -> Result<[u8; 32], String> {
        if password.len() < 12 {
            return Err("User wallet password must be at least 12 characters".into());
        }

        let mut key = [0u8; 32];

        let params = Params::new(
            ARGON2_MEMORY_KIB,
            ARGON2_ITERATIONS,
            ARGON2_PARALLELISM,
            Some(key.len()),
        )
        .map_err(|error| format!("User wallet KDF parameters are invalid: {error}"))?;

        Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
            .hash_password_into(password.as_bytes(), salt, &mut key)
            .map_err(|error| format!("User wallet encryption key could not be derived: {error}"))?;

        Ok(key)
    }

    fn encrypt(password: &str, plaintext: &[u8]) -> Result<Vec<u8>, String> {
        let random_salt_material = Key::<ChaCha20Poly1305>::generate();

        let mut salt = [0u8; 16];
        salt.copy_from_slice(&random_salt_material.as_slice()[..16]);

        let key = Self::derive_key(password, &salt)?;

        let cipher = ChaCha20Poly1305::new_from_slice(&key)
            .map_err(|_| "User wallet encryption key is invalid".to_string())?;

        let nonce = Nonce::generate();

        let ciphertext = cipher
            .encrypt(
                &nonce,
                Payload {
                    msg: plaintext,
                    aad: USER_WALLET_AAD,
                },
            )
            .map_err(|_| "User wallet private key could not be encrypted".to_string())?;

        let envelope = EncryptedUserWallet {
            version: 1,
            kdf: "argon2id".to_string(),
            kdf_memory_kib: ARGON2_MEMORY_KIB,
            kdf_iterations: ARGON2_ITERATIONS,
            kdf_parallelism: ARGON2_PARALLELISM,
            cipher: "chacha20poly1305".to_string(),
            salt_hex: hex::encode(salt),
            nonce_hex: hex::encode(nonce.as_slice()),
            ciphertext_hex: hex::encode(ciphertext),
        };

        serde_json::to_vec_pretty(&envelope)
            .map_err(|error| format!("Encrypted user wallet JSON could not be created: {error}"))
    }

    fn decrypt(password: &str, encrypted: &[u8]) -> Result<Vec<u8>, String> {
        let envelope: EncryptedUserWallet = serde_json::from_slice(encrypted)
            .map_err(|_| "User wallet encrypted file format is invalid".to_string())?;

        if envelope.version != 1
            || envelope.kdf != "argon2id"
            || envelope.kdf_memory_kib != ARGON2_MEMORY_KIB
            || envelope.kdf_iterations != ARGON2_ITERATIONS
            || envelope.kdf_parallelism != ARGON2_PARALLELISM
            || envelope.cipher != "chacha20poly1305"
        {
            return Err("User wallet encryption format is not supported".into());
        }

        let salt = hex::decode(&envelope.salt_hex)
            .map_err(|_| "User wallet salt format is invalid".to_string())?;

        if salt.len() != 16 {
            return Err("User wallet salt length is invalid".into());
        }

        let nonce_bytes = hex::decode(&envelope.nonce_hex)
            .map_err(|_| "User wallet nonce format is invalid".to_string())?;

        let nonce_array: [u8; 12] = nonce_bytes
            .try_into()
            .map_err(|_| "User wallet nonce length is invalid".to_string())?;

        let ciphertext = hex::decode(&envelope.ciphertext_hex)
            .map_err(|_| "User wallet ciphertext format is invalid".to_string())?;

        let key = Self::derive_key(password, &salt)?;

        let cipher = ChaCha20Poly1305::new_from_slice(&key)
            .map_err(|_| "User wallet decryption key is invalid".to_string())?;

        let nonce = Nonce::from(nonce_array);

        cipher
            .decrypt(
                &nonce,
                Payload {
                    msg: ciphertext.as_ref(),
                    aad: USER_WALLET_AAD,
                },
            )
            .map_err(|_| "User wallet password is incorrect or the file is corrupted".to_string())
    }

    fn write_new(&self, contents: &[u8]) -> Result<(), String> {
        let final_path = self.path();

        let random_name = hex::encode(Key::<ChaCha20Poly1305>::generate());
        let temp_path = self
            .data_directory
            .join(format!(".user-wallet-{random_name}.tmp"));

        let write_result = Self::write_private_file(&temp_path, contents);

        if let Err(error) = write_result {
            let _ = fs::remove_file(&temp_path);
            return Err(error);
        }

        if let Err(error) = fs::hard_link(&temp_path, &final_path) {
            let _ = fs::remove_file(&temp_path);

            return Err(format!(
                "User wallet could not be moved to the active location or already exists: {error}"
            ));
        }

        let _ = fs::remove_file(&temp_path);

        Ok(())
    }

    fn write_private_file(path: &Path, contents: &[u8]) -> Result<(), String> {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);

        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }

        let mut file = options
            .open(path)
            .map_err(|error| format!("User wallet temp file could not be created: {error}"))?;

        file.write_all(contents)
            .map_err(|error| format!("User wallet file could not be written: {error}"))?;

        file.sync_all()
            .map_err(|error| format!("User wallet file could not be synchronized: {error}"))
    }
}

#[cfg(test)]
mod user_wallet_tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn test_directory(name: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock must be valid")
            .as_nanos();

        std::env::temp_dir().join(format!(
            "kybernetes-user-wallet-{name}-{}-{stamp}",
            std::process::id()
        ))
    }

    fn cleanup(path: &Path) {
        let _ = fs::remove_dir_all(path);
    }

    #[test]
    fn wallet_create_and_load_round_trip() {
        let directory = test_directory("round-trip");
        let keystore = UserWalletKeystore::at(&directory);
        let password = "wallet-test-password-123";

        let created = keystore
            .create(password)
            .expect("wallet creation must succeed");

        assert!(keystore.exists());

        let loaded = keystore
            .load(password)
            .expect("wallet load must succeed")
            .expect("wallet must exist");

        assert_eq!(created.address(), loaded.address());
        assert_eq!(created.public_key_hex(), loaded.public_key_hex());

        cleanup(&directory);
    }

    #[test]
    fn wrong_password_is_rejected() {
        let directory = test_directory("wrong-password");
        let keystore = UserWalletKeystore::at(&directory);

        keystore
            .create("correct-wallet-password-123")
            .expect("wallet creation must succeed");

        let result = keystore.load("incorrect-wallet-password-456");

        assert!(result.is_err());

        cleanup(&directory);
    }

    #[test]
    fn existing_wallet_is_not_overwritten() {
        let directory = test_directory("no-overwrite");
        let keystore = UserWalletKeystore::at(&directory);
        let password = "wallet-test-password-123";

        let original = keystore
            .create(password)
            .expect("first wallet creation must succeed");

        let second = keystore.create(password);

        assert!(second.is_err());

        let loaded = keystore
            .load(password)
            .expect("original wallet must remain readable")
            .expect("original wallet must still exist");

        assert_eq!(original.address(), loaded.address());

        cleanup(&directory);
    }

    #[test]
    fn keystore_does_not_store_plaintext_private_key_or_address() {
        let directory = test_directory("encrypted-file");
        let keystore = UserWalletKeystore::at(&directory);
        let password = "wallet-test-password-123";

        let wallet = keystore
            .create(password)
            .expect("wallet creation must succeed");

        let contents = fs::read_to_string(keystore.path()).expect("keystore file must be readable");

        assert!(!contents.contains(&wallet.private_key_hex()));
        assert!(!contents.contains(wallet.address()));

        cleanup(&directory);
    }

    #[test]
    fn short_password_is_rejected() {
        let directory = test_directory("short-password");
        let keystore = UserWalletKeystore::at(&directory);

        let result = keystore.create("short");

        assert!(result.is_err());
        assert!(!keystore.exists());

        cleanup(&directory);
    }
}
