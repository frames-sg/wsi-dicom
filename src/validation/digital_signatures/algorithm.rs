//! The MAC Algorithms of PS3.3 Table C.12.1.1.3.1.2-1, by Defined Term and by
//! the object identifier that PKCS #1 DigestInfo and RFC 3161 tokens use.
//! PS3.15 C.1 profiles RIPEMD160, MD5, SHA1, SHA256, SHA384 and SHA512, and
//! its validators accept them all; the other terms are verified alike.

use rsa::Pkcs1v15Sign;
use sha2::digest::const_oid::{AssociatedOid, ObjectIdentifier};
use sha2::digest::{Digest, DynDigest};

macro_rules! mac_algorithms {
    ($($algorithm:ident: $term:literal => $digest:ty,)+) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub(super) enum MacAlgorithm {
            $($algorithm,)+
        }

        impl MacAlgorithm {
            pub(super) const ALL: &'static [Self] = &[$(Self::$algorithm,)+];

            /// The Defined Term of MAC Algorithm (0400,0015).
            pub(super) fn term(self) -> &'static str {
                match self {
                    $(Self::$algorithm => $term,)+
                }
            }

            pub(super) fn oid(self) -> ObjectIdentifier {
                match self {
                    $(Self::$algorithm => <$digest as AssociatedOid>::OID,)+
                }
            }

            /// A hasher to feed incrementally.
            pub(super) fn hasher(self) -> Box<dyn DynDigest> {
                match self {
                    $(Self::$algorithm => Box::new(<$digest>::new()),)+
                }
            }

            /// RSASSA-PKCS1-v1_5 over this hash's DigestInfo (PS3.15 C.1).
            pub(super) fn padding(self) -> Pkcs1v15Sign {
                match self {
                    $(Self::$algorithm => Pkcs1v15Sign::new::<$digest>(),)+
                }
            }
        }
    };
}

mac_algorithms! {
    Ripemd160: "RIPEMD160" => ripemd::Ripemd160,
    Md5: "MD5" => md5::Md5,
    Sha1: "SHA1" => sha1::Sha1,
    Sha224: "SHA224" => sha2::Sha224,
    Sha256: "SHA256" => sha2::Sha256,
    Sha384: "SHA384" => sha2::Sha384,
    Sha512: "SHA512" => sha2::Sha512,
    Sha512_224: "SHA512_224" => sha2::Sha512_224,
    Sha512_256: "SHA512_256" => sha2::Sha512_256,
    Sha3_224: "SHA3_224" => sha3::Sha3_224,
    Sha3_256: "SHA3_256" => sha3::Sha3_256,
    Sha3_384: "SHA3_384" => sha3::Sha3_384,
    Sha3_512: "SHA3_512" => sha3::Sha3_512,
}

impl MacAlgorithm {
    pub(super) fn of_term(term: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|algorithm| algorithm.term() == term)
    }

    pub(super) fn of_oid(oid: ObjectIdentifier) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|algorithm| algorithm.oid() == oid)
    }

    pub(super) fn digest(self, bytes: &[u8]) -> Box<[u8]> {
        let mut hasher = self.hasher();
        hasher.update(bytes);
        hasher.finalize()
    }
}
