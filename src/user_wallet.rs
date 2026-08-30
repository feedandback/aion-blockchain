use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

#[cfg(not(windows))]
use std::fs::OpenOptions;

use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::{
    ChaCha20Poly1305, KeyInit, Nonce,
    aead::{Aead, Generate, Key, Payload},
};

use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

use crate::wallet::Wallet;

pub const USER_WALLET_FILE_NAME: &str = "user-wallet.json";

const USER_WALLET_FORMAT: &str = "kybernetes-user-wallet-v1";
const USER_WALLET_AAD: &[u8] = b"KYBERNETES_USER_WALLET_V1";

const ARGON2_MEMORY_KIB: u32 = 19_456;
const ARGON2_ITERATIONS: u32 = 2;
const ARGON2_PARALLELISM: u32 = 1;

#[derive(serde::Serialize, serde::Deserialize, ZeroizeOnDrop)]
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

        #[cfg(windows)]
        Self::restrict_windows_directory_acl(&self.data_directory)?;

        let wallet = Wallet::new();

        let stored = StoredUserWallet {
            format: USER_WALLET_FORMAT.to_string(),
            address: wallet.address().to_string(),
            private_key_hex: wallet.private_key_hex(),
        };

        let mut plaintext = serde_json::to_vec(&stored)
            .map_err(|error| format!("User wallet data could not be serialized: {error}"))?;

        let encrypted_result = Self::encrypt(password, &plaintext);
        plaintext.zeroize();
        let encrypted = encrypted_result?;

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

        let mut plaintext = Self::decrypt(password, &encrypted)?;

        let stored_result = serde_json::from_slice::<StoredUserWallet>(&plaintext);
        plaintext.zeroize();

        let stored = stored_result.map_err(|_| "User wallet data format is invalid".to_string())?;

        if stored.format != USER_WALLET_FORMAT {
            return Err("User wallet format is not supported".into());
        }

        let wallet = Wallet::from_private_key_hex(&stored.private_key_hex)?;

        if wallet.address() != stored.address {
            return Err("User wallet address does not match the private key".into());
        }

        Ok(Some(wallet))
    }

    fn derive_key(password: &str, salt: &[u8]) -> Result<Zeroizing<[u8; 32]>, String> {
        if password.len() < 12 {
            return Err("User wallet password must be at least 12 characters".into());
        }

        let mut key = Zeroizing::new([0u8; 32]);

        let params = Params::new(
            ARGON2_MEMORY_KIB,
            ARGON2_ITERATIONS,
            ARGON2_PARALLELISM,
            Some(key.len()),
        )
        .map_err(|error| format!("User wallet KDF parameters are invalid: {error}"))?;

        Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
            .hash_password_into(password.as_bytes(), salt, &mut key[..])
            .map_err(|error| format!("User wallet encryption key could not be derived: {error}"))?;

        Ok(key)
    }

    fn encrypt(password: &str, plaintext: &[u8]) -> Result<Vec<u8>, String> {
        let random_salt_material = Key::<ChaCha20Poly1305>::generate();

        let mut salt = [0u8; 16];
        salt.copy_from_slice(&random_salt_material.as_slice()[..16]);

        let key = Self::derive_key(password, &salt)?;

        let cipher = ChaCha20Poly1305::new_from_slice(&key[..])
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

        let cipher = ChaCha20Poly1305::new_from_slice(&key[..])
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
        #[cfg(windows)]
        let data_directory = fs::canonicalize(&self.data_directory)
            .map_err(|error| format!("User wallet directory could not be resolved: {error}"))?;

        #[cfg(not(windows))]
        let data_directory = &self.data_directory;

        let final_path = data_directory.join(USER_WALLET_FILE_NAME);

        let random_name = hex::encode(Key::<ChaCha20Poly1305>::generate());
        let temp_path = data_directory.join(format!(".user-wallet-{random_name}.tmp"));

        let temp_file = match Self::write_private_file(&temp_path, contents) {
            Ok(file) => file,
            Err(error) => {
                let _ = fs::remove_file(&temp_path);
                return Err(error);
            }
        };

        #[cfg(not(windows))]
        drop(temp_file);

        let publish_result = fs::hard_link(&temp_path, &final_path);

        #[cfg(windows)]
        drop(temp_file);

        if let Err(error) = publish_result {
            let _ = fs::remove_file(&temp_path);

            return Err(format!(
                "User wallet could not be moved to the active location or already exists: {error}"
            ));
        }

        let _ = fs::remove_file(&temp_path);

        Ok(())
    }

    #[cfg(windows)]
    fn create_private_file(path: &Path) -> Result<fs::File, String> {
        use std::mem::size_of;
        use std::os::windows::ffi::OsStrExt;
        use std::os::windows::io::FromRawHandle;
        use std::ptr::null_mut;

        use windows_sys::Win32::Foundation::{GENERIC_WRITE, INVALID_HANDLE_VALUE, LocalFree};
        use windows_sys::Win32::Security::Authorization::{
            ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
        };
        use windows_sys::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};
        use windows_sys::Win32::Storage::FileSystem::{
            CREATE_NEW, CreateFileW, FILE_ATTRIBUTE_NORMAL,
        };

        let mut wide_path: Vec<u16> = path.as_os_str().encode_wide().collect();

        if wide_path.contains(&0) {
            return Err("User wallet temp file path contains an interior null character".into());
        }

        wide_path.push(0);

        // Protected DACL:
        // - OW = current object owner
        // - SY = Local System
        // - BA = Built-in Administrators
        let sddl: Vec<u16> = "D:P(A;;FA;;;OW)(A;;FA;;;SY)(A;;FA;;;BA)\0"
            .encode_utf16()
            .collect();

        let mut security_descriptor: PSECURITY_DESCRIPTOR = null_mut();

        let converted = unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                SDDL_REVISION_1,
                &mut security_descriptor,
                null_mut(),
            )
        };

        if converted == 0 {
            return Err(format!(
                "User wallet Windows security descriptor could not be created: {}",
                std::io::Error::last_os_error()
            ));
        }

        let security_attributes = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: security_descriptor,
            bInheritHandle: 0,
        };

        let handle = unsafe {
            CreateFileW(
                wide_path.as_ptr(),
                GENERIC_WRITE,
                0, // Keep the temporary file exclusively open while it is initialized.
                &security_attributes,
                CREATE_NEW,
                FILE_ATTRIBUTE_NORMAL,
                null_mut(),
            )
        };

        let create_error = if handle == INVALID_HANDLE_VALUE {
            Some(std::io::Error::last_os_error())
        } else {
            None
        };

        unsafe {
            LocalFree(security_descriptor);
        }

        if let Some(error) = create_error {
            return Err(format!(
                "User wallet temp file could not be created securely: {error}"
            ));
        }

        Ok(unsafe { fs::File::from_raw_handle(handle) })
    }

    #[cfg(windows)]
    fn restrict_windows_directory_acl(path: &Path) -> Result<(), String> {
        use std::os::windows::ffi::OsStrExt;
        use std::ptr::null_mut;

        use windows_sys::Win32::Foundation::LocalFree;
        use windows_sys::Win32::Security::Authorization::{
            ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
        };
        use windows_sys::Win32::Security::{
            DACL_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR,
            SetFileSecurityW,
        };

        let resolved = fs::canonicalize(path)
            .map_err(|error| format!("User wallet directory could not be resolved: {error}"))?;

        let mut wide_path: Vec<u16> = resolved.as_os_str().encode_wide().collect();

        if wide_path.contains(&0) {
            return Err("User wallet directory path contains an interior null character".into());
        }

        wide_path.push(0);

        // Protect the Kybernetes data directory from inherited mutation rights.
        // Full control remains with the object owner, Local System, and Administrators.
        let sddl: Vec<u16> = "D:P(A;;FA;;;OW)(A;;FA;;;SY)(A;;FA;;;BA)\0"
            .encode_utf16()
            .collect();

        let mut security_descriptor: PSECURITY_DESCRIPTOR = null_mut();

        let converted = unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                SDDL_REVISION_1,
                &mut security_descriptor,
                null_mut(),
            )
        };

        if converted == 0 {
            return Err(format!(
                "User wallet directory security descriptor could not be created: {}",
                std::io::Error::last_os_error()
            ));
        }

        let result = unsafe {
            SetFileSecurityW(
                wide_path.as_ptr(),
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                security_descriptor,
            )
        };

        let set_error = if result == 0 {
            Some(std::io::Error::last_os_error())
        } else {
            None
        };

        unsafe {
            LocalFree(security_descriptor);
        }

        if let Some(error) = set_error {
            return Err(format!(
                "User wallet directory permissions could not be restricted: {error}"
            ));
        }

        Ok(())
    }
    #[cfg(not(windows))]
    fn create_private_file(path: &Path) -> Result<fs::File, String> {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);

        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }

        options
            .open(path)
            .map_err(|error| format!("User wallet temp file could not be created: {error}"))
    }

    fn write_private_file(path: &Path, contents: &[u8]) -> Result<fs::File, String> {
        let mut file = Self::create_private_file(path)?;

        file.write_all(contents)
            .map_err(|error| format!("User wallet file could not be written: {error}"))?;

        file.sync_all()
            .map_err(|error| format!("User wallet file could not be synchronized: {error}"))?;

        Ok(file)
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

    #[cfg(windows)]
    #[test]
    fn wallet_temp_file_is_locked_until_publication() {
        let directory = test_directory("locked-until-publication");
        fs::create_dir_all(&directory).expect("test directory must be created");

        let temp_path = directory.join("wallet.tmp");
        let renamed_path = directory.join("renamed-wallet.tmp");
        let final_path = directory.join(USER_WALLET_FILE_NAME);

        let temp_file = UserWalletKeystore::write_private_file(
            &temp_path,
            b"encrypted-wallet-regression-test-data",
        )
        .expect("protected temp file must be written");

        assert!(
            fs::remove_file(&temp_path).is_err(),
            "open wallet temp file must reject deletion"
        );
        assert!(
            fs::rename(&temp_path, &renamed_path).is_err(),
            "open wallet temp file must reject renaming"
        );

        fs::hard_link(&temp_path, &final_path)
            .expect("wallet must be publishable while its temp handle is open");

        drop(temp_file);

        fs::remove_file(&temp_path).expect("temp link must be removable after publication");
        assert!(final_path.exists());

        cleanup(&directory);
    }

    #[cfg(windows)]
    #[test]
    fn wallet_keystore_has_protected_dacl() {
        use std::mem::size_of;
        use std::os::windows::ffi::OsStrExt;
        use std::ptr::null_mut;

        use windows_sys::Win32::Security::{
            DACL_SECURITY_INFORMATION, GetFileSecurityW, GetSecurityDescriptorControl,
            SE_DACL_PROTECTED,
        };

        let directory = test_directory("protected-dacl");
        let keystore = UserWalletKeystore::at(&directory);

        keystore
            .create("wallet-test-password-123")
            .expect("wallet creation must succeed");

        let wide_path: Vec<u16> = keystore
            .path()
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let mut required_length = 0;

        unsafe {
            GetFileSecurityW(
                wide_path.as_ptr(),
                DACL_SECURITY_INFORMATION,
                null_mut(),
                0,
                &mut required_length,
            );
        }

        assert!(
            required_length > 0,
            "wallet security descriptor size must be available: {}",
            std::io::Error::last_os_error()
        );

        let descriptor_words = required_length.div_ceil(size_of::<u32>() as u32) as usize;
        let mut security_descriptor = vec![0u32; descriptor_words];
        let security_result = unsafe {
            GetFileSecurityW(
                wide_path.as_ptr(),
                DACL_SECURITY_INFORMATION,
                security_descriptor.as_mut_ptr().cast(),
                required_length,
                &mut required_length,
            )
        };

        assert_ne!(
            security_result,
            0,
            "wallet security descriptor must be readable: {}",
            std::io::Error::last_os_error()
        );

        let mut control = 0;
        let mut revision = 0;
        let control_result = unsafe {
            GetSecurityDescriptorControl(
                security_descriptor.as_mut_ptr().cast(),
                &mut control,
                &mut revision,
            )
        };

        assert_ne!(
            control_result,
            0,
            "wallet security descriptor control must be readable: {}",
            std::io::Error::last_os_error()
        );
        assert_ne!(
            control & SE_DACL_PROTECTED,
            0,
            "wallet DACL must be protected from inheritance"
        );

        cleanup(&directory);
    }

    #[cfg(windows)]
    #[test]
    fn wallet_directory_has_protected_dacl() {
        use std::mem::size_of;
        use std::os::windows::ffi::OsStrExt;
        use std::ptr::null_mut;

        use windows_sys::Win32::Security::{
            DACL_SECURITY_INFORMATION, GetFileSecurityW, GetSecurityDescriptorControl,
            SE_DACL_PROTECTED,
        };

        let directory = test_directory("protected-directory-dacl");
        let keystore = UserWalletKeystore::at(&directory);

        keystore
            .create("wallet-test-password-123")
            .expect("wallet creation must succeed");

        let wide_path: Vec<u16> = directory
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();

        let mut required_length = 0;

        unsafe {
            GetFileSecurityW(
                wide_path.as_ptr(),
                DACL_SECURITY_INFORMATION,
                null_mut(),
                0,
                &mut required_length,
            );
        }

        assert!(
            required_length > 0,
            "wallet directory security descriptor size must be available: {}",
            std::io::Error::last_os_error()
        );

        let descriptor_words = required_length.div_ceil(size_of::<u32>() as u32) as usize;
        let mut security_descriptor = vec![0u32; descriptor_words];

        let security_result = unsafe {
            GetFileSecurityW(
                wide_path.as_ptr(),
                DACL_SECURITY_INFORMATION,
                security_descriptor.as_mut_ptr().cast(),
                required_length,
                &mut required_length,
            )
        };

        assert_ne!(
            security_result,
            0,
            "wallet directory security descriptor must be readable: {}",
            std::io::Error::last_os_error()
        );

        let mut control = 0;
        let mut revision = 0;

        let control_result = unsafe {
            GetSecurityDescriptorControl(
                security_descriptor.as_mut_ptr().cast(),
                &mut control,
                &mut revision,
            )
        };

        assert_ne!(
            control_result,
            0,
            "wallet directory security descriptor control must be readable: {}",
            std::io::Error::last_os_error()
        );

        assert_ne!(
            control & SE_DACL_PROTECTED,
            0,
            "wallet data directory DACL must be protected from inheritance"
        );

        cleanup(&directory);
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
