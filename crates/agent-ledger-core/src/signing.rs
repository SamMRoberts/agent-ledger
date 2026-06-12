use std::{fs, path::Path};

use anyhow::{anyhow, Context};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rand::rngs::OsRng;

pub struct SessionKey {
    signing_key: SigningKey,
}

impl SessionKey {
    pub fn generate() -> Self {
        let mut rng = OsRng;
        Self {
            signing_key: SigningKey::generate(&mut rng),
        }
    }

    pub fn sign(&self, data: &[u8]) -> Signature {
        self.signing_key.sign(data)
    }

    pub fn public_key(&self) -> VerifyingKey {
        self.signing_key.verifying_key()
    }

    pub fn public_key_hex(&self) -> String {
        hex::encode(self.public_key().to_bytes())
    }

    pub fn save_to_file(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, hex::encode(self.signing_key.to_bytes()))
            .with_context(|| format!("writing session key to {}", path.display()))?;
        Ok(())
    }

    pub fn load_from_file(path: &Path) -> anyhow::Result<Self> {
        let hex_data = fs::read_to_string(path)
            .with_context(|| format!("reading session key from {}", path.display()))?;
        let bytes = hex::decode(hex_data.trim())?;
        let secret: [u8; 32] = bytes
            .try_into()
            .map_err(|_| anyhow!("expected 32-byte ed25519 secret key"))?;
        Ok(Self {
            signing_key: SigningKey::from_bytes(&secret),
        })
    }
}

pub fn verify_signature(
    public_key: &VerifyingKey,
    data: &[u8],
    signature: &[u8],
) -> anyhow::Result<()> {
    let signature_bytes: [u8; 64] = signature
        .try_into()
        .map_err(|_| anyhow!("expected 64-byte ed25519 signature"))?;
    let signature = Signature::from_bytes(&signature_bytes);
    public_key.verify(data, &signature)?;
    Ok(())
}
