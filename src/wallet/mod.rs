use ed25519_dalek::{
    Signature,
    Signer,
    SigningKey,
    Verifier,
    VerifyingKey,
};
use rand_core::OsRng;
use sha2::{
    Digest,
    Sha256,
};

#[derive(Debug)]
pub struct Wallet {
    signing_key: SigningKey,
    verifying_key: VerifyingKey,
    address: String,
}

impl Wallet {
    pub fn new() -> Self {
        let mut rng = OsRng;

        let signing_key =
            SigningKey::generate(
                &mut rng,
            );

        Self::from_signing_key(
            signing_key,
        )
    }

    fn from_signing_key(
        signing_key: SigningKey,
    ) -> Self {
        let verifying_key =
            signing_key
                .verifying_key();

        let address =
            Self::address_from_public_key_bytes(
                verifying_key
                    .as_bytes(),
            );

        Self {
            signing_key,
            verifying_key,
            address,
        }
    }

    pub fn from_private_key_hex(
        private_key_hex: &str,
    ) -> Result<Self, String> {
        let private_key_bytes =
            hex::decode(
                private_key_hex,
            )
            .map_err(
                |_| {
                    "Private key hex formatı geçersiz"
                        .to_string()
                },
            )?;

        let private_key_array:
            [u8; 32] =
            private_key_bytes
                .try_into()
                .map_err(
                    |_| {
                        "Private key 32 byte olmalı"
                            .to_string()
                    },
                )?;

        let signing_key =
            SigningKey::from_bytes(
                &private_key_array,
            );

        Ok(
            Self::from_signing_key(
                signing_key,
            ),
        )
    }

    pub fn private_key_hex(
        &self,
    ) -> String {
        hex::encode(
            self.signing_key
                .to_bytes(),
        )
    }

    pub fn address(
        &self,
    ) -> &str {
        &self.address
    }

    pub fn public_key_hex(
        &self,
    ) -> String {
        hex::encode(
            self.verifying_key
                .as_bytes(),
        )
    }

    pub fn sign(
        &self,
        message: &[u8],
    ) -> String {
        let signature =
            self.signing_key
                .sign(message);

        hex::encode(
            signature.to_bytes(),
        )
    }

    pub fn address_from_public_key(
        public_key_hex: &str,
    ) -> Option<String> {
        let public_key_bytes =
            hex::decode(
                public_key_hex,
            )
            .ok()?;

        let public_key_array:
            [u8; 32] =
            public_key_bytes
                .try_into()
                .ok()?;

        VerifyingKey::from_bytes(
            &public_key_array,
        )
        .ok()?;

        Some(
            Self::address_from_public_key_bytes(
                &public_key_array,
            ),
        )
    }

    pub fn verify(
        public_key_hex: &str,
        message: &[u8],
        signature_hex: &str,
    ) -> bool {
        let public_key_bytes =
            match hex::decode(
                public_key_hex,
            ) {
                Ok(bytes) => bytes,
                Err(_) => {
                    return false;
                }
            };

        let signature_bytes =
            match hex::decode(
                signature_hex,
            ) {
                Ok(bytes) => bytes,
                Err(_) => {
                    return false;
                }
            };

        let public_key_array:
            [u8; 32] =
            match public_key_bytes
                .try_into()
            {
                Ok(bytes) => bytes,
                Err(_) => {
                    return false;
                }
            };

        let signature_array:
            [u8; 64] =
            match signature_bytes
                .try_into()
            {
                Ok(bytes) => bytes,
                Err(_) => {
                    return false;
                }
            };

        let verifying_key =
            match VerifyingKey::from_bytes(
                &public_key_array,
            ) {
                Ok(key) => key,
                Err(_) => {
                    return false;
                }
            };

        let signature =
            Signature::from_bytes(
                &signature_array,
            );

        verifying_key
            .verify(
                message,
                &signature,
            )
            .is_ok()
    }

    fn address_from_public_key_bytes(
        public_key_bytes: &[u8],
    ) -> String {
        let mut hasher =
            Sha256::new();

        hasher.update(
            public_key_bytes,
        );

        hex::encode(
            hasher.finalize(),
        )
    }
}