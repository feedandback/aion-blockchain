use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::{
    ChaCha20Poly1305, KeyInit, Nonce,
    aead::{Aead, Generate, Key, Payload},
};

use crate::consensus::Consensus;
use crate::protocol::NETWORK_ID;
use crate::storage::Storage;
use crate::wallet::Wallet;

const VALIDATOR_KEYSTORE_FILE_NAME: &str = "validator-keystore.json";
const VALIDATOR_KEYSTORE_FORMAT: &str = "kybernetes-validator-key-v1";
const VALIDATOR_KEYSTORE_AAD: &[u8] = b"KYBERNETES_VALIDATOR_KEYSTORE_V1";
const VALIDATOR_CANDIDATE_FILE_NAME: &str = "validator-candidate.json";
const VALIDATOR_CANDIDATE_FORMAT: &str = "kybernetes-validator-candidate-v1";
const VALIDATOR_CANDIDATE_AAD: &[u8] = b"KYBERNETES_VALIDATOR_CANDIDATE_V1";
const VALIDATOR_CANDIDATE_NETWORK_FAMILY: &str = "kybernetes";
const MAX_VALIDATOR_KEYSTORE_BYTES: u64 = 1024 * 1024;
const ARGON2_MEMORY_KIB: u32 = 19_456;
const ARGON2_ITERATIONS: u32 = 2;
const ARGON2_PARALLELISM: u32 = 1;

#[derive(serde::Serialize, serde::Deserialize)]
struct EncryptedValidatorKeystore {
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

#[derive(serde::Serialize, serde::Deserialize)]
struct StoredValidatorKey {
    format: String,
    network_id: String,
    genesis_fingerprint: String,
    validator_address: String,
    private_key_hex: String,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct StoredValidatorCandidate {
    format: String,
    network_family: String,
    validator_address: String,
    private_key_hex: String,
}

pub struct ValidatorIdentity {
    wallet: Wallet,
}

pub struct ValidatorCandidate {
    wallet: Wallet,
}

impl ValidatorCandidate {
    pub fn address(&self) -> &str {
        self.wallet.address()
    }
}

pub struct ValidatorActivation {
    validator_address: String,
    candidate_removed: bool,
}

impl ValidatorActivation {
    pub fn address(&self) -> &str {
        &self.validator_address
    }

    pub fn candidate_removed(&self) -> bool {
        self.candidate_removed
    }
}

pub struct ValidatorCandidateKeystore {
    data_directory: PathBuf,
}

impl ValidatorCandidateKeystore {
    pub fn configured() -> Self {
        Self::at(Storage::data_directory())
    }

    pub fn at(data_directory: impl Into<PathBuf>) -> Self {
        Self {
            data_directory: data_directory.into(),
        }
    }

    pub fn path(&self) -> PathBuf {
        self.data_directory.join(VALIDATOR_CANDIDATE_FILE_NAME)
    }

    pub fn exists(&self) -> Result<bool, String> {
        ValidatorKeystore::path_exists(&self.path(), "Validator candidate keystore")
    }

    pub fn generate(&self, password: &str) -> Result<String, String> {
        if self.exists()? {
            return Err(
                "Validator candidate already exists; automatic overwrite was rejected".into(),
            );
        }

        let wallet = Wallet::new();
        let validator_address = wallet.address().to_string();
        let stored_candidate = StoredValidatorCandidate {
            format: VALIDATOR_CANDIDATE_FORMAT.to_string(),
            network_family: VALIDATOR_CANDIDATE_NETWORK_FAMILY.to_string(),
            validator_address: validator_address.clone(),
            private_key_hex: wallet.private_key_hex(),
        };
        let plaintext = serde_json::to_vec(&stored_candidate).map_err(|error| {
            format!("Validator candidate payload could not be created: {error}")
        })?;
        let serialized = ValidatorKeystore::encrypt(password, &plaintext, VALIDATOR_CANDIDATE_AAD)?;

        ValidatorKeystore::store_without_overwrite(
            &self.data_directory,
            &self.path(),
            ".validator-candidate",
            &serialized,
        )?;

        Ok(validator_address)
    }

    pub fn load(&self, password: &str) -> Result<Option<ValidatorCandidate>, String> {
        let encrypted = match ValidatorKeystore::read_encrypted_file(
            &self.path(),
            "Validator candidate keystore",
        )? {
            Some(encrypted) => encrypted,
            None => return Ok(None),
        };
        let plaintext = ValidatorKeystore::decrypt(password, &encrypted, VALIDATOR_CANDIDATE_AAD)?;
        let stored_candidate: StoredValidatorCandidate = serde_json::from_slice(&plaintext)
            .map_err(|_| "Validator candidate payload is invalid".to_string())?;

        if stored_candidate.format != VALIDATOR_CANDIDATE_FORMAT
            || stored_candidate.network_family != VALIDATOR_CANDIDATE_NETWORK_FAMILY
        {
            return Err("Validator candidate does not match the network family or format".into());
        }

        let wallet = Wallet::from_private_key_hex(&stored_candidate.private_key_hex)?;
        if wallet.address() != stored_candidate.validator_address {
            return Err("Validator candidate address does not match the private key".into());
        }

        Ok(Some(ValidatorCandidate { wallet }))
    }

    pub fn activate(
        &self,
        password: &str,
        consensus: &Consensus,
        genesis_fingerprint: &str,
    ) -> Result<ValidatorActivation, String> {
        let active_keystore = ValidatorKeystore::at(&self.data_directory);
        if active_keystore.exists()? {
            return Err(
                "Validator keystore already exists; automatic overwrite was rejected".into(),
            );
        }

        let candidate = self
            .load(password)?
            .ok_or("Validator candidate keystore was not found")?;
        let validator_address = candidate.address().to_string();
        let private_key_hex = candidate.wallet.private_key_hex();
        active_keystore.provision(password, &private_key_hex, consensus, genesis_fingerprint)?;

        let candidate_removed = fs::remove_file(self.path()).is_ok();

        Ok(ValidatorActivation {
            validator_address,
            candidate_removed,
        })
    }
}

impl ValidatorIdentity {
    pub fn from_private_key(private_key_hex: &str, consensus: &Consensus) -> Result<Self, String> {
        let wallet = Wallet::from_private_key_hex(private_key_hex)?;

        if !consensus.is_validator_allowed(wallet.address()) {
            return Err("Private key is not in the canonical validator set".into());
        }

        Ok(Self { wallet })
    }

    pub fn address(&self) -> &str {
        self.wallet.address()
    }

    pub(crate) fn wallet(&self) -> &Wallet {
        &self.wallet
    }
}

pub struct ValidatorKeystore {
    data_directory: PathBuf,
}

impl ValidatorKeystore {
    pub fn configured() -> Self {
        Self::at(Storage::data_directory())
    }

    pub fn at(data_directory: impl Into<PathBuf>) -> Self {
        Self {
            data_directory: data_directory.into(),
        }
    }

    pub fn path(&self) -> PathBuf {
        self.data_directory.join(VALIDATOR_KEYSTORE_FILE_NAME)
    }

    pub fn exists(&self) -> Result<bool, String> {
        Self::path_exists(&self.path(), "Validator keystore")
    }

    fn path_exists(path: &Path, label: &str) -> Result<bool, String> {
        match fs::metadata(path) {
            Ok(_) => Ok(true),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(format!("Could not determine {label} access state: {error}")),
        }
    }

    pub fn provision(
        &self,
        password: &str,
        private_key_hex: &str,
        consensus: &Consensus,
        genesis_fingerprint: &str,
    ) -> Result<String, String> {
        let identity = ValidatorIdentity::from_private_key(private_key_hex, consensus)?;
        let validator_address = identity.address().to_string();
        let final_path = self.path();

        if self.exists()? {
            return Err(
                "Validator keystore already exists; automatic overwrite was rejected".into(),
            );
        }

        let stored_key = StoredValidatorKey {
            format: VALIDATOR_KEYSTORE_FORMAT.to_string(),
            network_id: NETWORK_ID.to_string(),
            genesis_fingerprint: genesis_fingerprint.to_string(),
            validator_address: validator_address.clone(),
            private_key_hex: private_key_hex.to_string(),
        };
        let plaintext = serde_json::to_vec(&stored_key)
            .map_err(|error| format!("Validator key payload could not be created: {error}"))?;
        let serialized = Self::encrypt(password, &plaintext, VALIDATOR_KEYSTORE_AAD)?;
        Self::store_without_overwrite(
            &self.data_directory,
            &final_path,
            ".validator-keystore",
            &serialized,
        )?;

        Ok(validator_address)
    }

    pub fn load_authorized(
        &self,
        password: &str,
        consensus: &Consensus,
        expected_genesis_fingerprint: &str,
    ) -> Result<Option<ValidatorIdentity>, String> {
        let encrypted = match Self::read_encrypted_file(&self.path(), "Validator keystore")? {
            Some(encrypted) => encrypted,
            None => return Ok(None),
        };

        let plaintext = Self::decrypt(password, &encrypted, VALIDATOR_KEYSTORE_AAD)?;
        let stored_key: StoredValidatorKey = serde_json::from_slice(&plaintext)
            .map_err(|_| "Validator keystore payload is invalid".to_string())?;

        if stored_key.format != VALIDATOR_KEYSTORE_FORMAT
            || stored_key.network_id != NETWORK_ID
            || stored_key.genesis_fingerprint != expected_genesis_fingerprint
        {
            return Err("Validator keystore does not match the network or genesis identity".into());
        }

        let identity = ValidatorIdentity::from_private_key(&stored_key.private_key_hex, consensus)?;

        if identity.address() != stored_key.validator_address {
            return Err("Validator keystore address does not match the private key".into());
        }

        Ok(Some(identity))
    }

    fn derive_key(password: &str, salt: &[u8]) -> Result<[u8; 32], String> {
        if password.len() < 12 {
            return Err("Validator keystore password must be at least 12 characters".into());
        }

        let mut key = [0u8; 32];
        let params = Params::new(
            ARGON2_MEMORY_KIB,
            ARGON2_ITERATIONS,
            ARGON2_PARALLELISM,
            Some(key.len()),
        )
        .map_err(|error| format!("Validator keystore KDF parameters are invalid: {error}"))?;
        Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
            .hash_password_into(password.as_bytes(), salt, &mut key)
            .map_err(|error| format!("Validator keystore key could not be derived: {error}"))?;
        Ok(key)
    }

    fn encrypt(password: &str, plaintext: &[u8], aad: &[u8]) -> Result<Vec<u8>, String> {
        let random_salt_material = Key::<ChaCha20Poly1305>::generate();
        let mut salt = [0u8; 16];
        salt.copy_from_slice(&random_salt_material.as_slice()[..16]);
        let key = Self::derive_key(password, &salt)?;
        let cipher = ChaCha20Poly1305::new_from_slice(&key)
            .map_err(|_| "Validator keystore encryption key is invalid".to_string())?;
        let nonce = Nonce::generate();
        let ciphertext = cipher
            .encrypt(
                &nonce,
                Payload {
                    msg: plaintext,
                    aad,
                },
            )
            .map_err(|_| "Validator private key could not be encrypted".to_string())?;
        let envelope = EncryptedValidatorKeystore {
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
            .map_err(|error| format!("Validator keystore JSON could not be created: {error}"))
    }

    fn decrypt(password: &str, encrypted: &[u8], aad: &[u8]) -> Result<Vec<u8>, String> {
        let envelope: EncryptedValidatorKeystore = serde_json::from_slice(encrypted)
            .map_err(|_| "Validator keystore format is invalid".to_string())?;

        if envelope.version != 1
            || envelope.kdf != "argon2id"
            || envelope.kdf_memory_kib != ARGON2_MEMORY_KIB
            || envelope.kdf_iterations != ARGON2_ITERATIONS
            || envelope.kdf_parallelism != ARGON2_PARALLELISM
            || envelope.cipher != "chacha20poly1305"
        {
            return Err("Validator keystore encryption format is not supported".into());
        }

        let salt = hex::decode(&envelope.salt_hex)
            .map_err(|_| "Validator keystore salt format is invalid".to_string())?;
        if salt.len() != 16 {
            return Err("Validator keystore salt length is invalid".into());
        }

        let nonce_bytes = hex::decode(&envelope.nonce_hex)
            .map_err(|_| "Validator keystore nonce format is invalid".to_string())?;
        let nonce_array: [u8; 12] = nonce_bytes
            .try_into()
            .map_err(|_| "Validator keystore nonce length is invalid".to_string())?;
        let ciphertext = hex::decode(&envelope.ciphertext_hex)
            .map_err(|_| "Validator keystore ciphertext format is invalid".to_string())?;
        let key = Self::derive_key(password, &salt)?;
        let cipher = ChaCha20Poly1305::new_from_slice(&key)
            .map_err(|_| "Validator keystore decryption key is invalid".to_string())?;
        let nonce = Nonce::from(nonce_array);

        cipher
            .decrypt(
                &nonce,
                Payload {
                    msg: ciphertext.as_ref(),
                    aad,
                },
            )
            .map_err(|_| {
                "Validator keystore password is incorrect or the file is corrupted".to_string()
            })
    }

    fn read_encrypted_file(path: &Path, label: &str) -> Result<Option<Vec<u8>>, String> {
        let file = match File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(format!("{label} could not be opened: {error}")),
        };
        let metadata = file
            .metadata()
            .map_err(|error| format!("{label} metadata could not be read: {error}"))?;

        if metadata.len() == 0 || metadata.len() > MAX_VALIDATOR_KEYSTORE_BYTES {
            return Err(format!("{label} size is invalid"));
        }

        let mut encrypted = Vec::new();
        file.take(MAX_VALIDATOR_KEYSTORE_BYTES + 1)
            .read_to_end(&mut encrypted)
            .map_err(|error| format!("{label} could not be read: {error}"))?;
        if encrypted.len() as u64 > MAX_VALIDATOR_KEYSTORE_BYTES {
            return Err(format!("{label} size is invalid"));
        }

        Ok(Some(encrypted))
    }

    fn store_without_overwrite(
        data_directory: &Path,
        final_path: &Path,
        temp_prefix: &str,
        serialized: &[u8],
    ) -> Result<(), String> {
        fs::create_dir_all(data_directory).map_err(|error| {
            format!(
                "Validator keystore data directory could not be created ({}): {error}",
                data_directory.display()
            )
        })?;

        let temp_path = data_directory.join(format!(
            "{}-{}.tmp",
            temp_prefix,
            hex::encode(Key::<ChaCha20Poly1305>::generate().as_slice())
        ));
        let write_result = Self::write_private_file(&temp_path, serialized);

        if let Err(error) = write_result {
            let _ = fs::remove_file(&temp_path);
            return Err(error);
        }

        if let Err(error) = fs::hard_link(&temp_path, final_path) {
            let _ = fs::remove_file(&temp_path);
            return Err(format!(
                "Validator keystore could not be moved to the active location or already exists: {error}"
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

        let mut file = options.open(path).map_err(|error| {
            format!("Validator keystore temp file could not be created: {error}")
        })?;
        file.write_all(contents)
            .map_err(|error| format!("Validator keystore could not be written: {error}"))?;
        file.sync_all().map_err(|error| {
            format!("Validator keystore could not be synchronized to disk: {error}")
        })
    }
}
