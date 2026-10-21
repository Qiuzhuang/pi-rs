//! PKCE helpers (port of `auth/oauth/pkce.ts`).

use base64::Engine;
use rand::RngCore;
use sha2::{Digest, Sha256};

/// Base64url without padding.
pub fn base64url_encode(bytes: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// A PKCE verifier and its S256 challenge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pkce {
    pub verifier: String,
    pub challenge: String,
}

/// The S256 challenge of a verifier.
pub fn pkce_challenge(verifier: &str) -> String {
    base64url_encode(&Sha256::digest(verifier.as_bytes()))
}

/// Generate a PKCE verifier (32 random bytes, base64url) and challenge.
pub fn generate_pkce() -> Pkce {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    let verifier = base64url_encode(&bytes);
    let challenge = pkce_challenge(&verifier);
    Pkce {
        verifier,
        challenge,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc7636_example() {
        assert_eq!(
            pkce_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
        let pkce = generate_pkce();
        assert_eq!(pkce.verifier.len(), 43);
        assert!(!pkce.verifier.contains(['+', '/', '=']));
        assert_eq!(pkce.challenge, pkce_challenge(&pkce.verifier));
    }
}
