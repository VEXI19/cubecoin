use std::fmt;

use bech32::{Bech32m, Hrp};
use sha2::{Digest, Sha256};

pub const ADDRESS_HRP: &str = "cube";
pub const ADDRESS_VERSION: u8 = 1;
pub const ADDRESS_DOMAIN_TAG: &[u8] = b"cubecoin/address/ed25519/v1\0";

const PUBLIC_KEY_LENGTH: usize = 32;
const ADDRESS_DIGEST_LENGTH: usize = 32;
const ADDRESS_PAYLOAD_LENGTH: usize = 1 + ADDRESS_DIGEST_LENGTH;
const ADDRESS_HRP_VALUE: Hrp = Hrp::parse_unchecked(ADDRESS_HRP);

/// A versioned CubeCoin wallet address derived from an Ed25519 public key.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Address([u8; ADDRESS_PAYLOAD_LENGTH]);

impl Address {
    /// Derives an address from canonical 32-byte Ed25519 public-key encoding.
    pub fn from_ed25519_public_key(public_key: &[u8; PUBLIC_KEY_LENGTH]) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(ADDRESS_DOMAIN_TAG);
        hasher.update(public_key);

        let mut payload = [0; ADDRESS_PAYLOAD_LENGTH];
        payload[0] = ADDRESS_VERSION;
        payload[1..].copy_from_slice(&hasher.finalize());

        Self(payload)
    }
}

impl fmt::Display for Address {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        bech32::encode_to_fmt::<Bech32m, _>(formatter, ADDRESS_HRP_VALUE, &self.0)
            .map_err(|_| fmt::Error)
    }
}

#[cfg(test)]
mod tests {
    use bech32::{Bech32m, Hrp, primitives::decode::CheckedHrpstring};

    use super::{ADDRESS_HRP, ADDRESS_VERSION, Address};

    #[test]
    fn derives_the_specified_address_for_a_known_public_key() {
        let public_key = [
            0xd7, 0x5a, 0x98, 0x01, 0x82, 0xb1, 0x0a, 0xb7, 0xd5, 0x4b, 0xfe, 0xd3, 0xc9, 0x64,
            0x07, 0x3a, 0x0e, 0xe1, 0x72, 0xf3, 0xda, 0xa6, 0x23, 0x25, 0xaf, 0x02, 0x1a, 0x68,
            0xf7, 0x07, 0x51, 0x1a,
        ];

        let address = Address::from_ed25519_public_key(&public_key);

        assert_eq!(
            address.to_string(),
            "cube1qx447me5a3glzxu7tm7rt0fk3w2d0lzpjzla0wafh5ung6um3cgcv6etrp7"
        );
    }

    #[test]
    fn encodes_a_versioned_bech32m_payload() {
        let address = Address::from_ed25519_public_key(&[0; 32]).to_string();
        let checked = CheckedHrpstring::new::<Bech32m>(&address).expect("address uses Bech32m");
        let payload: Vec<u8> = checked.byte_iter().collect();

        assert_eq!(
            checked.hrp(),
            Hrp::parse(ADDRESS_HRP).expect("valid address HRP")
        );
        assert_eq!(payload.len(), 33);
        assert_eq!(payload[0], ADDRESS_VERSION);
    }

    #[test]
    fn public_keys_produce_distinct_addresses() {
        assert_ne!(
            Address::from_ed25519_public_key(&[0; 32]),
            Address::from_ed25519_public_key(&[1; 32])
        );
    }
}
