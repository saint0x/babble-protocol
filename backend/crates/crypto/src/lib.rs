use babble_types::{Error, Result};
use ed25519_dalek::{Signature as DalekSignature, Signer, SigningKey, Verifier, VerifyingKey};
use rand_core::OsRng;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub enum SignatureAlgorithm {
    Ed25519,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PublicKey {
    pub algorithm: SignatureAlgorithm,
    pub bytes: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Signature {
    pub algorithm: SignatureAlgorithm,
    pub bytes: String,
}

#[derive(Clone)]
pub struct Keypair {
    signing: SigningKey,
}

impl Keypair {
    pub fn generate() -> Self {
        Self {
            signing: SigningKey::generate(&mut OsRng),
        }
    }

    pub fn from_ed25519_secret_hex(value: &str) -> Result<Self> {
        let bytes = hex::decode(value.trim()).map_err(|_| Error::Signature)?;
        let seed: [u8; 32] = bytes.try_into().map_err(|_| Error::Signature)?;
        Ok(Self {
            signing: SigningKey::from_bytes(&seed),
        })
    }

    pub fn ed25519_secret_hex(&self) -> String {
        hex::encode(self.signing.to_bytes())
    }

    pub fn public_key(&self) -> PublicKey {
        PublicKey {
            algorithm: SignatureAlgorithm::Ed25519,
            bytes: hex::encode(self.signing.verifying_key().as_bytes()),
        }
    }

    pub fn sign(&self, payload: &[u8]) -> Signature {
        Signature {
            algorithm: SignatureAlgorithm::Ed25519,
            bytes: hex::encode(self.signing.sign(payload).to_bytes()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keypair_round_trips_ed25519_secret_material() {
        let keypair = Keypair::generate();
        let restored = Keypair::from_ed25519_secret_hex(&keypair.ed25519_secret_hex()).unwrap();
        let payload = b"babble signed payload";
        let signature = restored.sign(payload);

        assert_eq!(restored.public_key(), keypair.public_key());
        keypair.public_key().verify(payload, &signature).unwrap();
    }

    #[test]
    fn keypair_rejects_invalid_secret_hex() {
        assert!(Keypair::from_ed25519_secret_hex("not hex").is_err());
        assert!(Keypair::from_ed25519_secret_hex("abcd").is_err());
    }

    #[test]
    fn public_key_rejects_wrong_signature() {
        let keypair = Keypair::generate();
        let other = Keypair::generate();
        let signature = other.sign(b"payload");

        assert!(keypair.public_key().verify(b"payload", &signature).is_err());
    }
}

impl PublicKey {
    pub fn verify(&self, payload: &[u8], signature: &Signature) -> Result<()> {
        match (&self.algorithm, &signature.algorithm) {
            (SignatureAlgorithm::Ed25519, SignatureAlgorithm::Ed25519) => {
                let key_bytes = hex::decode(&self.bytes).map_err(|_| Error::Signature)?;
                let sig_bytes = hex::decode(&signature.bytes).map_err(|_| Error::Signature)?;
                let key_array: [u8; 32] = key_bytes.try_into().map_err(|_| Error::Signature)?;
                let sig_array: [u8; 64] = sig_bytes.try_into().map_err(|_| Error::Signature)?;
                let verifying =
                    VerifyingKey::from_bytes(&key_array).map_err(|_| Error::Signature)?;
                let sig = DalekSignature::from_bytes(&sig_array);
                verifying
                    .verify(payload, &sig)
                    .map_err(|_| Error::Signature)
            }
        }
    }
}
