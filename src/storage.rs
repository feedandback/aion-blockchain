use std::fs::{
    self,
    File,
};
use std::io::{
    Read,
    Write,
};
use std::path::PathBuf;

use argon2::Argon2;
use chacha20poly1305::{
    aead::{
        Aead,
        Generate,
        Key,
        KeyInit,
    },
    ChaCha20Poly1305,
    Nonce,
};

use crate::core::Block;

const DEFAULT_DATA_DIRECTORY: &str =
    "data";

const DATA_DIRECTORY_ENV: &str =
    "AION_DATA_DIR";

const BLOCKCHAIN_FILE_NAME: &str =
    "blockchain.json";

const BLOCKCHAIN_TEMP_FILE_NAME: &str =
    "blockchain.tmp";

const WALLETS_FILE_NAME: &str =
    "wallets.json";

const WALLETS_TEMP_FILE_NAME: &str =
    "wallets.tmp";

#[derive(
    serde::Serialize,
    serde::Deserialize,
)]
struct StoredWallets {
    alice_private_key: String,
    bob_private_key: String,
}

#[derive(
    serde::Serialize,
    serde::Deserialize,
)]
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
    pub fn data_directory(
    ) -> PathBuf {
        match std::env::var(
            DATA_DIRECTORY_ENV,
        ) {
            Ok(value)
                if !value
                    .trim()
                    .is_empty() =>
            {
                PathBuf::from(
                    value,
                )
            }

            _ => {
                PathBuf::from(
                    DEFAULT_DATA_DIRECTORY,
                )
            }
        }
    }

    pub fn blockchain_path(
    ) -> PathBuf {
        Self::data_directory()
            .join(
                BLOCKCHAIN_FILE_NAME,
            )
    }

    fn blockchain_temp_path(
    ) -> PathBuf {
        Self::data_directory()
            .join(
                BLOCKCHAIN_TEMP_FILE_NAME,
            )
    }

    pub fn wallets_path(
    ) -> PathBuf {
        Self::data_directory()
            .join(
                WALLETS_FILE_NAME,
            )
    }

    fn wallets_temp_path(
    ) -> PathBuf {
        Self::data_directory()
            .join(
                WALLETS_TEMP_FILE_NAME,
            )
    }

    fn ensure_data_directory(
    ) -> Result<(), String> {
        let data_directory =
            Self::data_directory();

        fs::create_dir_all(
            &data_directory,
        )
        .map_err(
            |error| {
                format!(
                    "Data klasörü oluşturulamadı ({}): {}",
                    data_directory.display(),
                    error
                )
            },
        )
    }

    pub fn save_blockchain(
        chain: &[Block],
    ) -> Result<(), String> {
        if chain.is_empty() {
            return Err(
                "Boş blockchain kaydedilemez"
                    .into(),
            );
        }

        Self::ensure_data_directory()?;

        let serialized =
            serde_json::to_vec_pretty(
                chain,
            )
            .map_err(
                |error| {
                    format!(
                        "Blockchain JSON'a çevrilemedi: {}",
                        error
                    )
                },
            )?;

        let temp_path =
            Self::blockchain_temp_path();

        let final_path =
            Self::blockchain_path();

        {
            let mut file =
                File::create(
                    &temp_path,
                )
                .map_err(
                    |error| {
                        format!(
                            "Geçici blockchain dosyası oluşturulamadı: {}",
                            error
                        )
                    },
                )?;

            file.write_all(
                &serialized,
            )
            .map_err(
                |error| {
                    format!(
                        "Blockchain diske yazılamadı: {}",
                        error
                    )
                },
            )?;

            file.sync_all()
                .map_err(
                    |error| {
                        format!(
                            "Blockchain dosyası diske senkronize edilemedi: {}",
                            error
                        )
                    },
                )?;
        }

        if final_path.exists() {
            fs::remove_file(
                &final_path,
            )
            .map_err(
                |error| {
                    format!(
                        "Eski blockchain dosyası silinemedi: {}",
                        error
                    )
                },
            )?;
        }

        fs::rename(
            &temp_path,
            &final_path,
        )
        .map_err(
            |error| {
                format!(
                    "Blockchain dosyası aktif konuma taşınamadı: {}",
                    error
                )
            },
        )?;

        Ok(())
    }

    pub fn load_blockchain(
    ) -> Result<
        Option<Vec<Block>>,
        String,
    > {
        let path =
            Self::blockchain_path();

        if !path.exists() {
            return Ok(None);
        }

        let mut file =
            File::open(
                &path,
            )
            .map_err(
                |error| {
                    format!(
                        "Blockchain dosyası açılamadı: {}",
                        error
                    )
                },
            )?;

        let mut bytes =
            Vec::new();

        file.read_to_end(
            &mut bytes,
        )
        .map_err(
            |error| {
                format!(
                    "Blockchain dosyası okunamadı: {}",
                    error
                )
            },
        )?;

        if bytes.is_empty() {
            return Err(
                "Blockchain dosyası boş"
                    .into(),
            );
        }

        let chain:
            Vec<Block> =
            serde_json::from_slice(
                &bytes,
            )
            .map_err(
                |error| {
                    format!(
                        "Blockchain dosyası geçersiz JSON: {}",
                        error
                    )
                },
            )?;

        if chain.is_empty() {
            return Err(
                "Diskteki blockchain boş"
                    .into(),
            );
        }

        Ok(
            Some(chain),
        )
    }

    fn derive_wallet_key(
        password: &str,
        salt: &[u8],
    ) -> Result<[u8; 32], String> {
        if password.len() < 12 {
            return Err(
                "Wallet şifresi en az 12 karakter olmalı"
                    .into(),
            );
        }

        let mut key =
            [0u8; 32];

        Argon2::default()
            .hash_password_into(
                password.as_bytes(),
                salt,
                &mut key,
            )
            .map_err(
                |error| {
                    format!(
                        "Wallet şifre anahtarı türetilemedi: {}",
                        error
                    )
                },
            )?;

        Ok(key)
    }

    pub fn save_wallet_private_keys(
        password: &str,
        alice_private_key: &str,
        bob_private_key: &str,
    ) -> Result<(), String> {
        Self::ensure_data_directory()?;

        let stored_wallets =
            StoredWallets {
                alice_private_key:
                    alice_private_key
                        .to_string(),
                bob_private_key:
                    bob_private_key
                        .to_string(),
            };

        let plaintext =
            serde_json::to_vec(
                &stored_wallets,
            )
            .map_err(
                |error| {
                    format!(
                        "Wallet verileri JSON'a çevrilemedi: {}",
                        error
                    )
                },
            )?;

        let random_salt_material =
            Key::<ChaCha20Poly1305>::generate();

        let mut salt =
            [0u8; 16];

        salt.copy_from_slice(
            &random_salt_material
                .as_slice()[..16],
        );

        let key =
            Self::derive_wallet_key(
                password,
                &salt,
            )?;

        let cipher =
            ChaCha20Poly1305::new_from_slice(
                &key,
            )
            .map_err(
                |_| {
                    "Wallet şifreleme anahtarı geçersiz"
                        .to_string()
                },
            )?;

        let nonce =
            Nonce::generate();

        let ciphertext =
            cipher
                .encrypt(
                    &nonce,
                    plaintext.as_ref(),
                )
                .map_err(
                    |_| {
                        "Wallet private key'leri şifrelenemedi"
                            .to_string()
                    },
                )?;

        let encrypted_wallets =
            EncryptedStoredWallets {
                version: 1,
                kdf: "argon2id".to_string(),
                cipher:
                    "chacha20poly1305"
                        .to_string(),
                salt_hex:
                    hex::encode(
                        salt,
                    ),
                nonce_hex:
                    hex::encode(
                        nonce.as_slice(),
                    ),
                ciphertext_hex:
                    hex::encode(
                        ciphertext,
                    ),
            };

        let serialized =
            serde_json::to_vec_pretty(
                &encrypted_wallets,
            )
            .map_err(
                |error| {
                    format!(
                        "Şifreli wallet verileri JSON'a çevrilemedi: {}",
                        error
                    )
                },
            )?;

        let temp_path =
            Self::wallets_temp_path();

        let final_path =
            Self::wallets_path();

        {
            let mut file =
                File::create(
                    &temp_path,
                )
                .map_err(
                    |error| {
                        format!(
                            "Geçici wallet dosyası oluşturulamadı: {}",
                            error
                        )
                    },
                )?;

            file.write_all(
                &serialized,
            )
            .map_err(
                |error| {
                    format!(
                        "Şifreli wallet verileri diske yazılamadı: {}",
                        error
                    )
                },
            )?;

            file.sync_all()
                .map_err(
                    |error| {
                        format!(
                            "Wallet dosyası diske senkronize edilemedi: {}",
                            error
                        )
                    },
                )?;
        }

        if final_path.exists() {
            fs::remove_file(
                &final_path,
            )
            .map_err(
                |error| {
                    format!(
                        "Eski wallet dosyası silinemedi: {}",
                        error
                    )
                },
            )?;
        }

        fs::rename(
            &temp_path,
            &final_path,
        )
        .map_err(
            |error| {
                format!(
                    "Wallet dosyası aktif konuma taşınamadı: {}",
                    error
                )
            },
        )?;

        Ok(())
    }

    pub fn load_wallet_private_keys(
        password: &str,
    ) -> Result<
        Option<(String, String)>,
        String,
    > {
        let path =
            Self::wallets_path();

        if !path.exists() {
            return Ok(None);
        }

        let mut file =
            File::open(
                &path,
            )
            .map_err(
                |error| {
                    format!(
                        "Wallet dosyası açılamadı: {}",
                        error
                    )
                },
            )?;

        let mut bytes =
            Vec::new();

        file.read_to_end(
            &mut bytes,
        )
        .map_err(
            |error| {
                format!(
                    "Wallet dosyası okunamadı: {}",
                    error
                )
            },
        )?;

        if bytes.is_empty() {
            return Err(
                "Wallet dosyası boş"
                    .into(),
            );
        }

        if let Ok(
            encrypted_wallets,
        ) =
            serde_json::from_slice::<
                EncryptedStoredWallets,
            >(&bytes)
        {
            if encrypted_wallets.version
                != 1
            {
                return Err(
                    "Wallet dosyası sürümü desteklenmiyor"
                        .into(),
                );
            }

            if encrypted_wallets.kdf
                != "argon2id"
                || encrypted_wallets.cipher
                    != "chacha20poly1305"
            {
                return Err(
                    "Wallet şifreleme formatı desteklenmiyor"
                        .into(),
                );
            }

            let salt =
                hex::decode(
                    &encrypted_wallets
                        .salt_hex,
                )
                .map_err(
                    |_| {
                        "Wallet salt formatı geçersiz"
                            .to_string()
                    },
                )?;

            if salt.len() != 16 {
                return Err(
                    "Wallet salt uzunluğu geçersiz"
                        .into(),
                );
            }

            let nonce_bytes =
                hex::decode(
                    &encrypted_wallets
                        .nonce_hex,
                )
                .map_err(
                    |_| {
                        "Wallet nonce formatı geçersiz"
                            .to_string()
                    },
                )?;

            let nonce_array:
                [u8; 12] =
                nonce_bytes
                    .try_into()
                    .map_err(
                        |_| {
                            "Wallet nonce uzunluğu geçersiz"
                                .to_string()
                        },
                    )?;

            let ciphertext =
                hex::decode(
                    &encrypted_wallets
                        .ciphertext_hex,
                )
                .map_err(
                    |_| {
                        "Wallet ciphertext formatı geçersiz"
                            .to_string()
                    },
                )?;

            let key =
                Self::derive_wallet_key(
                    password,
                    &salt,
                )?;

            let cipher =
                ChaCha20Poly1305::new_from_slice(
                    &key,
                )
                .map_err(
                    |_| {
                        "Wallet çözme anahtarı geçersiz"
                            .to_string()
                    },
                )?;

            let nonce =
                Nonce::from(
                    nonce_array,
                );

            let plaintext =
                cipher
                    .decrypt(
                        &nonce,
                        ciphertext.as_ref(),
                    )
                    .map_err(
                        |_| {
                            "Wallet şifresi yanlış veya wallet dosyası bozulmuş"
                                .to_string()
                        },
                    )?;

            let stored_wallets:
                StoredWallets =
                serde_json::from_slice(
                    &plaintext,
                )
                .map_err(
                    |error| {
                        format!(
                            "Çözülen wallet verileri geçersiz: {}",
                            error
                        )
                    },
                )?;

            return Ok(
                Some((
                    stored_wallets
                        .alice_private_key,
                    stored_wallets
                        .bob_private_key,
                )),
            );
        }

        // Eski düz metin wallets.json dosyasını tek seferlik taşı.
        let legacy_wallets:
            StoredWallets =
            serde_json::from_slice(
                &bytes,
            )
            .map_err(
                |_| {
                    "Wallet dosyası geçersiz veya desteklenmeyen formatta"
                        .to_string()
                },
            )?;

        Self::save_wallet_private_keys(
            password,
            &legacy_wallets
                .alice_private_key,
            &legacy_wallets
                .bob_private_key,
        )?;

        println!(
            "🔒 Eski düz metin wallet dosyası şifreli formata taşındı."
        );

        Ok(
            Some((
                legacy_wallets
                    .alice_private_key,
                legacy_wallets
                    .bob_private_key,
            )),
        )
    }

    pub fn blockchain_exists(
    ) -> bool {
        Self::blockchain_path()
            .exists()
    }

    pub fn delete_blockchain(
    ) -> Result<(), String> {
        let path =
            Self::blockchain_path();

        if !path.exists() {
            return Ok(());
        }

        fs::remove_file(
            path,
        )
        .map_err(
            |error| {
                format!(
                    "Blockchain dosyası silinemedi: {}",
                    error
                )
            },
        )
    }
}