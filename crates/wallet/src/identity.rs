use cubecoin_core::identity::Address;
use ed25519_dalek::SigningKey;
use zeroize::Zeroizing;

pub struct WalletIdentity {
    signing_key: SigningKey,
}

impl WalletIdentity {
    pub fn generate() -> Result<Self, getrandom::Error> {
        let mut seed = Zeroizing::new([0_u8; 32]);
        getrandom::fill(&mut *seed)?;

        Ok(Self {
            signing_key: SigningKey::from_bytes(&seed),
        })
    }

    pub fn address(&self) -> Address {
        Address::from_ed25519_public_key(&self.signing_key.verifying_key().to_bytes())
    }

    pub(crate) fn from_seed(seed: &[u8; 32]) -> Self {
        Self {
            signing_key: SigningKey::from_bytes(seed),
        }
    }

    pub(crate) fn seed(&self) -> Zeroizing<[u8; 32]> {
        Zeroizing::new(self.signing_key.to_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::WalletIdentity;

    #[test]
    fn a_fixed_seed_generates_a_stable_address() {
        let identity = WalletIdentity::from_seed(&[0x42; 32]);

        assert_eq!(
            identity.address().to_string(),
            "cube1q90sgc4ya0sptvy35l8lq3funru76nwj3e3c8j859y9sz6e4g8v05qqh8ye"
        );
    }

    #[test]
    fn generated_identities_have_distinct_addresses() {
        let first = WalletIdentity::generate().expect("operating system entropy is available");
        let second = WalletIdentity::generate().expect("operating system entropy is available");

        assert_ne!(first.address(), second.address());
    }
}
