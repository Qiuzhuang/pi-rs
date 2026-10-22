//! Symmetric ciphers for `crypto.createCipheriv`/`createDecipheriv` (and WebCrypto AES):
//! AES-128/192/256 in CBC, ECB, CTR and GCM modes, DES-EDE3-CBC, ChaCha20 and
//! ChaCha20-Poly1305. Modes are implemented here on top of the RustCrypto block ciphers so they
//! can stream like OpenSSL (`update` returns output as soon as it is available).

use boa_engine::{Context, JsArgs, JsResult, JsValue};
use cipher::generic_array::GenericArray;
use cipher::{BlockDecrypt, BlockEncrypt, KeyInit, KeyIvInit, StreamCipher, StreamCipherSeek};
use universal_hash::UniversalHash;

use super::{bytes_arg, bytes_to_js, coded_error, native_object, with_native};
use crate::engine::{OpFn, arg_string, js_str};

/// A cipher failure with Node's message, code and error class.
#[derive(Debug, Clone)]
pub(crate) struct CipherError {
    pub kind: &'static str,
    pub message: String,
    pub code: Option<&'static str>,
}

impl CipherError {
    pub(crate) fn new(kind: &'static str, message: &str, code: Option<&'static str>) -> Self {
        Self { kind, message: message.into(), code }
    }
    fn bad_decrypt() -> Self {
        Self::new("Error", "error:1C800064:Provider routines::bad decrypt", Some("ERR_OSSL_BAD_DECRYPT"))
    }
    fn wrong_final() -> Self {
        Self::new("Error", "error:1C80006B:Provider routines::wrong final block length", Some("ERR_OSSL_WRONG_FINAL_BLOCK_LENGTH"))
    }
    fn auth() -> Self {
        Self::new("Error", "Unsupported state or unable to authenticate data", None)
    }
    pub(crate) fn throw(&self, ctx: &mut Context) -> boa_engine::JsError {
        let props: Vec<(&str, JsValue)> = self.code.map(|c| vec![("code", js_str(c))]).unwrap_or_default();
        coded_error(ctx, self.kind, &self.message, &props)
    }
}

type CResult<T> = Result<T, CipherError>;

enum Block {
    A128(aes::Aes128),
    A192(aes::Aes192),
    A256(aes::Aes256),
    Des3(des::TdesEde3),
}

impl Block {
    fn new(bits: usize, key: &[u8]) -> Self {
        match bits {
            128 => Block::A128(aes::Aes128::new_from_slice(key).expect("key length checked")),
            192 => Block::A192(aes::Aes192::new_from_slice(key).expect("key length checked")),
            256 => Block::A256(aes::Aes256::new_from_slice(key).expect("key length checked")),
            _ => Block::Des3(des::TdesEde3::new_from_slice(key).expect("key length checked")),
        }
    }
    fn size(&self) -> usize {
        if matches!(self, Block::Des3(_)) { 8 } else { 16 }
    }
    fn encrypt(&self, b: &mut [u8]) {
        match self {
            Block::A128(c) => c.encrypt_block(GenericArray::from_mut_slice(b)),
            Block::A192(c) => c.encrypt_block(GenericArray::from_mut_slice(b)),
            Block::A256(c) => c.encrypt_block(GenericArray::from_mut_slice(b)),
            Block::Des3(c) => c.encrypt_block(GenericArray::from_mut_slice(b)),
        }
    }
    fn decrypt(&self, b: &mut [u8]) {
        match self {
            Block::A128(c) => c.decrypt_block(GenericArray::from_mut_slice(b)),
            Block::A192(c) => c.decrypt_block(GenericArray::from_mut_slice(b)),
            Block::A256(c) => c.decrypt_block(GenericArray::from_mut_slice(b)),
            Block::Des3(c) => c.decrypt_block(GenericArray::from_mut_slice(b)),
        }
    }
}

/// Streaming MAC over (AAD, ciphertext) in GCM / ChaCha20-Poly1305 layout.
enum Mac {
    Ghash(Box<ghash::GHash>),
    Poly(Box<poly1305::Poly1305>),
}

struct AeadState {
    mac: Mac,
    aad_len: u64,
    ct_len: u64,
    /// Partial 16-byte block of AAD or ciphertext not yet absorbed.
    pending: Vec<u8>,
    in_ct: bool,
    tag_mask: [u8; 16],
    tag_len: usize,
    expected_tag: Option<Vec<u8>>,
    tag: Option<Vec<u8>>,
}

impl AeadState {
    fn absorb(&mut self, data: &[u8]) {
        self.pending.extend_from_slice(data);
        let full = self.pending.len() / 16 * 16;
        if full > 0 {
            let blocks: Vec<_> = self.pending[..full].chunks(16).map(|c| *GenericArray::from_slice(c)).collect();
            match &mut self.mac {
                Mac::Ghash(g) => g.update(&blocks),
                Mac::Poly(p) => p.update(&blocks),
            }
            self.pending.drain(..full);
        }
    }
    fn pad(&mut self) {
        if !self.pending.is_empty() {
            let mut block = [0u8; 16];
            block[..self.pending.len()].copy_from_slice(&self.pending);
            self.pending.clear();
            let b = *GenericArray::from_slice(&block);
            match &mut self.mac {
                Mac::Ghash(g) => g.update(&[b]),
                Mac::Poly(p) => p.update(&[b]),
            }
        }
    }
    fn add_aad(&mut self, aad: &[u8]) -> CResult<()> {
        if self.in_ct {
            return Err(CipherError::new("Error", "Unsupported state", None));
        }
        self.aad_len += aad.len() as u64;
        self.absorb(aad);
        Ok(())
    }
    fn add_ct(&mut self, ct: &[u8]) {
        if !self.in_ct {
            self.pad();
            self.in_ct = true;
        }
        self.ct_len += ct.len() as u64;
        self.absorb(ct);
    }
    fn finish(&mut self) -> [u8; 16] {
        self.pad();
        let mut lens = [0u8; 16];
        let tag = match &mut self.mac {
            Mac::Ghash(g) => {
                lens[..8].copy_from_slice(&(self.aad_len * 8).to_be_bytes());
                lens[8..].copy_from_slice(&(self.ct_len * 8).to_be_bytes());
                g.update(&[*GenericArray::from_slice(&lens)]);
                std::mem::replace(g, Box::new(ghash::GHash::new(&Default::default()))).finalize()
            }
            Mac::Poly(p) => {
                lens[..8].copy_from_slice(&self.aad_len.to_le_bytes());
                lens[8..].copy_from_slice(&self.ct_len.to_le_bytes());
                p.update(&[*GenericArray::from_slice(&lens)]);
                std::mem::replace(p, Box::new(poly1305::Poly1305::new(&Default::default()))).finalize()
            }
        };
        let mut out = [0u8; 16];
        for i in 0..16 {
            out[i] = tag[i] ^ self.tag_mask[i];
        }
        out
    }
}

enum Mode {
    Cbc { block: Block, iv: Vec<u8> },
    Ecb { block: Block },
    /// CTR with a 128-bit big-endian counter (OpenSSL), or GCM's 32-bit counter.
    Ctr { block: Block, counter: [u8; 16], ks: [u8; 16], used: usize, inc32: bool },
    ChaCha { cipher: Box<chacha20::ChaCha20> },
}

pub(crate) struct CipherState {
    encrypt: bool,
    mode: Mode,
    padding: bool,
    buf: Vec<u8>,
    aead: Option<AeadState>,
    finalized: bool,
}

/// `(key bits or 0 for chacha, mode)` for a Node cipher name.
fn lookup(name: &str) -> Option<(usize, &'static str)> {
    let n = name.to_ascii_lowercase();
    Some(match n.as_str() {
        "aes-128-cbc" | "aes128" => (128, "cbc"),
        "aes-192-cbc" | "aes192" => (192, "cbc"),
        "aes-256-cbc" | "aes256" => (256, "cbc"),
        "aes-128-ecb" => (128, "ecb"),
        "aes-192-ecb" => (192, "ecb"),
        "aes-256-ecb" => (256, "ecb"),
        "aes-128-ctr" => (128, "ctr"),
        "aes-192-ctr" => (192, "ctr"),
        "aes-256-ctr" => (256, "ctr"),
        "aes-128-gcm" | "id-aes128-gcm" => (128, "gcm"),
        "aes-192-gcm" | "id-aes192-gcm" => (192, "gcm"),
        "aes-256-gcm" | "id-aes256-gcm" => (256, "gcm"),
        "des-ede3-cbc" | "des3" => (192, "des3"),
        "chacha20" => (256, "chacha20"),
        "chacha20-poly1305" => (256, "chacha20-poly1305"),
        _ => return None,
    })
}

/// Cipher names reported by `crypto.getCiphers()`.
pub(crate) const CIPHERS: &[&str] = &[
    "aes-128-cbc", "aes-128-ctr", "aes-128-ecb", "aes-128-gcm", "aes-192-cbc", "aes-192-ctr", "aes-192-ecb", "aes-192-gcm",
    "aes-256-cbc", "aes-256-ctr", "aes-256-ecb", "aes-256-gcm", "aes128", "aes192", "aes256", "chacha20", "chacha20-poly1305",
    "des-ede3-cbc", "des3", "id-aes128-gcm", "id-aes192-gcm", "id-aes256-gcm",
];

/// `getCipherInfo`: `(keyLength, ivLength, blockSize, mode)`.
pub(crate) fn info(name: &str) -> Option<(usize, usize, usize, &'static str)> {
    let (bits, mode) = lookup(name)?;
    Some(match mode {
        "cbc" => (bits / 8, 16, 16, "cbc"),
        "ecb" => (bits / 8, 0, 16, "ecb"),
        "ctr" => (bits / 8, 16, 1, "ctr"),
        "gcm" => (bits / 8, 12, 1, "gcm"),
        "des3" => (24, 8, 8, "cbc"),
        "chacha20" => (32, 16, 1, "stream"),
        _ => (32, 12, 1, "stream"),
    })
}

impl CipherState {
    pub(crate) fn new(name: &str, key: &[u8], iv: Option<&[u8]>, encrypt: bool, auth_tag_len: Option<usize>) -> CResult<Self> {
        let Some((bits, mode)) = lookup(name) else {
            return Err(CipherError::new("Error", "Unknown cipher", Some("ERR_CRYPTO_UNKNOWN_CIPHER")));
        };
        let key_len = if mode == "des3" { 24 } else { bits / 8 };
        if key.len() != key_len {
            return Err(CipherError::new("RangeError", "Invalid key length", Some("ERR_CRYPTO_INVALID_KEYLEN")));
        }
        let bad_iv = || CipherError::new("TypeError", "Invalid initialization vector", Some("ERR_CRYPTO_INVALID_IV"));
        let iv = iv.unwrap_or(&[]);
        let mut aead = None;
        let mode = match mode {
            "cbc" | "des3" => {
                let block = Block::new(if mode == "des3" { 0 } else { bits }, key);
                if iv.len() != block.size() {
                    return Err(bad_iv());
                }
                Mode::Cbc { block, iv: iv.to_vec() }
            }
            "ecb" => {
                if !iv.is_empty() {
                    return Err(bad_iv());
                }
                Mode::Ecb { block: Block::new(bits, key) }
            }
            "ctr" => {
                if iv.len() != 16 {
                    return Err(bad_iv());
                }
                let mut counter = [0u8; 16];
                counter.copy_from_slice(iv);
                Mode::Ctr { block: Block::new(bits, key), counter, ks: [0; 16], used: 16, inc32: false }
            }
            "gcm" => {
                if iv.is_empty() {
                    return Err(bad_iv());
                }
                let tag_len = auth_tag_len.unwrap_or(16);
                if !(4..=16).contains(&tag_len) || (tag_len < 12 && tag_len != 4 && tag_len != 8) {
                    return Err(CipherError::new("TypeError", &format!("Invalid authentication tag length: {tag_len}"), Some("ERR_CRYPTO_INVALID_AUTH_TAG")));
                }
                let block = Block::new(bits, key);
                let mut h = [0u8; 16];
                block.encrypt(&mut h);
                let mut j0 = [0u8; 16];
                if iv.len() == 12 {
                    j0[..12].copy_from_slice(iv);
                    j0[15] = 1;
                } else {
                    let mut g = ghash::GHash::new(GenericArray::from_slice(&h));
                    g.update_padded(iv);
                    let mut lens = [0u8; 16];
                    lens[8..].copy_from_slice(&((iv.len() as u64) * 8).to_be_bytes());
                    g.update(&[*GenericArray::from_slice(&lens)]);
                    j0.copy_from_slice(&g.finalize());
                }
                let mut mask = j0;
                block.encrypt(&mut mask);
                let mut counter = j0;
                inc32(&mut counter);
                aead = Some(AeadState {
                    mac: Mac::Ghash(Box::new(ghash::GHash::new(GenericArray::from_slice(&h)))),
                    aad_len: 0,
                    ct_len: 0,
                    pending: Vec::new(),
                    in_ct: false,
                    tag_mask: mask,
                    tag_len,
                    expected_tag: None,
                    tag: None,
                });
                Mode::Ctr { block, counter, ks: [0; 16], used: 16, inc32: true }
            }
            "chacha20" => {
                if iv.len() != 16 {
                    return Err(bad_iv());
                }
                let counter = u32::from_le_bytes([iv[0], iv[1], iv[2], iv[3]]);
                let mut c = chacha20::ChaCha20::new(GenericArray::from_slice(key), GenericArray::from_slice(&iv[4..16]));
                c.seek(counter as u64 * 64);
                Mode::ChaCha { cipher: Box::new(c) }
            }
            _ => {
                // chacha20-poly1305 (RFC 8439)
                if iv.len() != 12 {
                    return Err(bad_iv());
                }
                let tag_len = auth_tag_len.unwrap_or(16);
                if !(1..=16).contains(&tag_len) {
                    return Err(CipherError::new("TypeError", &format!("Invalid authentication tag length: {tag_len}"), Some("ERR_CRYPTO_INVALID_AUTH_TAG")));
                }
                let mut c = chacha20::ChaCha20::new(GenericArray::from_slice(key), GenericArray::from_slice(iv));
                let mut poly_key = [0u8; 32];
                c.apply_keystream(&mut poly_key);
                c.seek(64u64);
                aead = Some(AeadState {
                    mac: Mac::Poly(Box::new(poly1305::Poly1305::new(GenericArray::from_slice(&poly_key)))),
                    aad_len: 0,
                    ct_len: 0,
                    pending: Vec::new(),
                    in_ct: false,
                    tag_mask: [0; 16],
                    tag_len,
                    expected_tag: None,
                    tag: None,
                });
                Mode::ChaCha { cipher: Box::new(c) }
            }
        };
        Ok(Self { encrypt, mode, padding: true, buf: Vec::new(), aead, finalized: false })
    }

    fn block_size(&self) -> usize {
        match &self.mode {
            Mode::Cbc { block, .. } | Mode::Ecb { block } => block.size(),
            _ => 1,
        }
    }

    fn stream(&mut self, data: &[u8]) -> Vec<u8> {
        let mut out = data.to_vec();
        match &mut self.mode {
            Mode::Ctr { block, counter, ks, used, inc32: is32 } => {
                for b in out.iter_mut() {
                    if *used == 16 {
                        *ks = *counter;
                        block.encrypt(ks);
                        if *is32 {
                            inc32(counter);
                        } else {
                            inc128(counter);
                        }
                        *used = 0;
                    }
                    *b ^= ks[*used];
                    *used += 1;
                }
            }
            Mode::ChaCha { cipher } => cipher.apply_keystream(&mut out),
            _ => unreachable!(),
        }
        out
    }

    fn blocks(&mut self, n: usize) -> Vec<u8> {
        let mut data: Vec<u8> = self.buf.drain(..n).collect();
        let encrypt = self.encrypt;
        match &mut self.mode {
            Mode::Cbc { block, iv } => {
                let bs = block.size();
                for chunk in data.chunks_mut(bs) {
                    if encrypt {
                        for (b, p) in chunk.iter_mut().zip(iv.iter()) {
                            *b ^= p;
                        }
                        block.encrypt(chunk);
                        iv.copy_from_slice(chunk);
                    } else {
                        let saved = chunk.to_vec();
                        block.decrypt(chunk);
                        for (b, p) in chunk.iter_mut().zip(iv.iter()) {
                            *b ^= p;
                        }
                        *iv = saved;
                    }
                }
            }
            Mode::Ecb { block } => {
                let bs = block.size();
                for chunk in data.chunks_mut(bs) {
                    if encrypt { block.encrypt(chunk) } else { block.decrypt(chunk) }
                }
            }
            _ => unreachable!(),
        }
        data
    }

    pub(crate) fn update(&mut self, data: &[u8]) -> CResult<Vec<u8>> {
        if self.finalized {
            return Err(CipherError::new("Error", "Unsupported state", Some("ERR_CRYPTO_INVALID_STATE")));
        }
        let bs = self.block_size();
        if bs == 1 {
            if !self.encrypt && let Some(a) = &mut self.aead {
                a.add_ct(data);
            }
            let out = self.stream(data);
            if self.encrypt && let Some(a) = &mut self.aead {
                a.add_ct(&out);
            }
            return Ok(out);
        }
        self.buf.extend_from_slice(data);
        let mut n = self.buf.len() / bs * bs;
        // Decrypting with padding keeps the last full block for `final` (it holds the padding).
        if !self.encrypt && self.padding && n == self.buf.len() && n > 0 {
            n -= bs;
        }
        Ok(if n > 0 { self.blocks(n) } else { Vec::new() })
    }

    pub(crate) fn finalize(&mut self) -> CResult<Vec<u8>> {
        if self.finalized {
            return Err(CipherError::new("Error", "Unsupported state", Some("ERR_CRYPTO_INVALID_STATE")));
        }
        self.finalized = true;
        let bs = self.block_size();
        if bs == 1 {
            if let Some(a) = &mut self.aead {
                let tag = a.finish();
                if self.encrypt {
                    a.tag = Some(tag[..a.tag_len].to_vec());
                } else {
                    let Some(expected) = &a.expected_tag else { return Err(CipherError::auth()) };
                    let got = &tag[..expected.len()];
                    if got.iter().zip(expected).fold(0u8, |d, (x, y)| d | (x ^ y)) != 0 {
                        return Err(CipherError::auth());
                    }
                }
            }
            return Ok(Vec::new());
        }
        if self.encrypt {
            if self.padding {
                let pad = bs - self.buf.len() % bs;
                self.buf.extend(std::iter::repeat_n(pad as u8, pad));
            } else if !self.buf.len().is_multiple_of(bs) {
                return Err(CipherError::wrong_final());
            }
            let n = self.buf.len();
            return Ok(self.blocks(n));
        }
        if !self.buf.len().is_multiple_of(bs) {
            return Err(CipherError::wrong_final());
        }
        if !self.padding {
            let n = self.buf.len();
            return Ok(self.blocks(n));
        }
        if self.buf.len() != bs {
            return Err(CipherError::wrong_final());
        }
        let mut last = self.blocks(bs);
        let pad = *last.last().unwrap_or(&0) as usize;
        if pad == 0 || pad > bs || !last[bs - pad..].iter().all(|&b| b as usize == pad) {
            return Err(CipherError::bad_decrypt());
        }
        last.truncate(bs - pad);
        Ok(last)
    }

    pub(crate) fn set_aad(&mut self, aad: &[u8]) -> CResult<()> {
        match &mut self.aead {
            Some(a) => a.add_aad(aad),
            None => Err(CipherError::new("Error", "Unsupported state", None)),
        }
    }

    pub(crate) fn auth_tag(&self) -> CResult<Vec<u8>> {
        match self.aead.as_ref().and_then(|a| a.tag.clone()) {
            Some(t) if self.encrypt => Ok(t),
            _ => Err(CipherError::new("Error", "Invalid state for operation getAuthTag", Some("ERR_CRYPTO_INVALID_STATE"))),
        }
    }

    pub(crate) fn set_auth_tag(&mut self, tag: &[u8]) -> CResult<()> {
        let encrypt = self.encrypt;
        match &mut self.aead {
            Some(a) if !encrypt => {
                let gcm = matches!(a.mac, Mac::Ghash(_));
                let ok = if gcm { (4..=16).contains(&tag.len()) && (tag.len() >= 12 || tag.len() == 4 || tag.len() == 8) } else { (1..=16).contains(&tag.len()) };
                if !ok {
                    return Err(CipherError::new("TypeError", &format!("Invalid authentication tag length: {}", tag.len()), Some("ERR_CRYPTO_INVALID_AUTH_TAG")));
                }
                a.expected_tag = Some(tag.to_vec());
                Ok(())
            }
            _ => Err(CipherError::new("Error", "Invalid state for operation setAuthTag", Some("ERR_CRYPTO_INVALID_STATE"))),
        }
    }
}

fn inc32(c: &mut [u8; 16]) {
    let n = u32::from_be_bytes([c[12], c[13], c[14], c[15]]).wrapping_add(1);
    c[12..].copy_from_slice(&n.to_be_bytes());
}

fn inc128(c: &mut [u8; 16]) {
    let n = u128::from_be_bytes(*c).wrapping_add(1);
    *c = n.to_be_bytes();
}

/// One-shot encryption/decryption (WebCrypto, tests).
pub(crate) fn one_shot(name: &str, key: &[u8], iv: Option<&[u8]>, encrypt: bool, data: &[u8]) -> CResult<Vec<u8>> {
    let mut c = CipherState::new(name, key, iv, encrypt, None)?;
    let mut out = c.update(data)?;
    out.extend(c.finalize()?);
    Ok(out)
}

/// AES key wrap (RFC 3394), for WebCrypto `AES-KW`.
pub(crate) fn aes_kw(key: &[u8], data: &[u8], wrap: bool) -> CResult<Vec<u8>> {
    let block = match key.len() {
        16 => Block::new(128, key),
        24 => Block::new(192, key),
        32 => Block::new(256, key),
        _ => return Err(CipherError::new("Error", "Invalid key length", None)),
    };
    let iv = [0xa6u8; 8];
    if wrap {
        if !data.len().is_multiple_of(8) || data.len() < 16 {
            return Err(CipherError::new("Error", "Invalid data length", None));
        }
        let n = data.len() / 8;
        let mut a = iv;
        let mut r: Vec<[u8; 8]> = data.chunks(8).map(|c| c.try_into().unwrap()).collect();
        for j in 0..6 {
            for (i, ri) in r.iter_mut().enumerate() {
                let mut b = [0u8; 16];
                b[..8].copy_from_slice(&a);
                b[8..].copy_from_slice(ri);
                block.encrypt(&mut b);
                let t = (n * j + i + 1) as u64;
                a.copy_from_slice(&b[..8]);
                for (x, y) in a.iter_mut().zip(t.to_be_bytes()) {
                    *x ^= y;
                }
                ri.copy_from_slice(&b[8..]);
            }
        }
        let mut out = a.to_vec();
        for ri in r {
            out.extend_from_slice(&ri);
        }
        Ok(out)
    } else {
        if !data.len().is_multiple_of(8) || data.len() < 24 {
            return Err(CipherError::new("Error", "Invalid data length", None));
        }
        let n = data.len() / 8 - 1;
        let mut a: [u8; 8] = data[..8].try_into().unwrap();
        let mut r: Vec<[u8; 8]> = data[8..].chunks(8).map(|c| c.try_into().unwrap()).collect();
        for j in (0..6).rev() {
            for i in (0..n).rev() {
                let t = (n * j + i + 1) as u64;
                for (x, y) in a.iter_mut().zip(t.to_be_bytes()) {
                    *x ^= y;
                }
                let mut b = [0u8; 16];
                b[..8].copy_from_slice(&a);
                b[8..].copy_from_slice(&r[i]);
                block.decrypt(&mut b);
                a.copy_from_slice(&b[..8]);
                r[i].copy_from_slice(&b[8..]);
            }
        }
        if a != iv {
            return Err(CipherError::new("Error", "Integrity check failed", None));
        }
        Ok(r.concat())
    }
}

// ---------------------------------------------------------------------------
// Ops
// ---------------------------------------------------------------------------

fn opt_bytes(v: &JsValue, ctx: &mut Context) -> JsResult<Option<Vec<u8>>> {
    if v.is_null_or_undefined() { Ok(None) } else { bytes_arg(v, ctx).map(Some) }
}

/// `ops.cipherNew(name, key, iv|null, encrypt, authTagLength?) -> handle`
fn op_cipher_new(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let name = arg_string(args, 0, ctx)?;
    let key = bytes_arg(args.get_or_undefined(1), ctx)?;
    let iv = opt_bytes(args.get_or_undefined(2), ctx)?;
    let encrypt = args.get_or_undefined(3).to_boolean();
    let tag_len = args.get_or_undefined(4);
    let tag_len = if tag_len.is_null_or_undefined() { None } else { Some(tag_len.to_number(ctx)? as usize) };
    match CipherState::new(&name, &key, iv.as_deref(), encrypt, tag_len) {
        Ok(s) => Ok(native_object(s)),
        Err(e) => Err(e.throw(ctx)),
    }
}

fn op_cipher_update(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let data = bytes_arg(args.get_or_undefined(1), ctx)?;
    match with_native::<CipherState, _>(args.get_or_undefined(0), |c| c.update(&data))? {
        Ok(out) => bytes_to_js(&out, ctx),
        Err(e) => Err(e.throw(ctx)),
    }
}

fn op_cipher_final(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    match with_native::<CipherState, _>(args.get_or_undefined(0), |c| c.finalize())? {
        Ok(out) => bytes_to_js(&out, ctx),
        Err(e) => Err(e.throw(ctx)),
    }
}

fn op_cipher_ctl(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let what = arg_string(args, 1, ctx)?;
    let arg = args.get_or_undefined(2).clone();
    let bytes = if what == "setAAD" || what == "setAuthTag" { bytes_arg(&arg, ctx)? } else { Vec::new() };
    let r = with_native::<CipherState, _>(args.get_or_undefined(0), |c| match what.as_str() {
        "setAAD" => c.set_aad(&bytes).map(|_| None),
        "setAuthTag" => c.set_auth_tag(&bytes).map(|_| None),
        "getAuthTag" => c.auth_tag().map(Some),
        "setAutoPadding" => {
            c.padding = arg.to_boolean();
            Ok(None)
        }
        _ => Ok(None),
    })?;
    match r {
        Ok(Some(b)) => bytes_to_js(&b, ctx),
        Ok(None) => Ok(JsValue::undefined()),
        Err(e) => Err(e.throw(ctx)),
    }
}

/// OpenSSL's canonical name and NID for `getCipherInfo`.
fn canonical(name: &str) -> (&'static str, u32) {
    let (bits, mode) = lookup(name).unwrap_or((0, ""));
    match (bits, mode) {
        (128, "cbc") => ("aes-128-cbc", 419),
        (192, "cbc") => ("aes-192-cbc", 423),
        (256, "cbc") => ("aes-256-cbc", 427),
        (128, "ecb") => ("aes-128-ecb", 418),
        (192, "ecb") => ("aes-192-ecb", 422),
        (256, "ecb") => ("aes-256-ecb", 426),
        (128, "ctr") => ("aes-128-ctr", 904),
        (192, "ctr") => ("aes-192-ctr", 905),
        (256, "ctr") => ("aes-256-ctr", 906),
        (128, "gcm") => ("id-aes128-gcm", 895),
        (192, "gcm") => ("id-aes192-gcm", 898),
        (256, "gcm") => ("id-aes256-gcm", 901),
        (_, "des3") => ("des-ede3-cbc", 44),
        (_, "chacha20") => ("chacha20", 1019),
        _ => ("chacha20-poly1305", 1018),
    }
}

/// `ops.cipherInfo(name) -> { name, nid, keyLength, ivLength, blockSize, mode } | undefined`
fn op_cipher_info(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let name = arg_string(args, 0, ctx)?;
    match info(&name) {
        Some((k, iv, bs, mode)) => {
            let (canonical, nid) = canonical(&name);
            super::object(
                ctx,
                vec![
                    ("name", js_str(canonical)),
                    ("nid", JsValue::from(nid)),
                    ("keyLength", JsValue::from(k as u32)),
                    ("ivLength", JsValue::from(iv as u32)),
                    ("blockSize", JsValue::from(bs as u32)),
                    ("mode", js_str(mode)),
                ],
            )
        }
        None => Ok(JsValue::undefined()),
    }
}

fn op_ciphers(_: &JsValue, _: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let items = CIPHERS.iter().map(|c| js_str(c)).collect();
    Ok(super::array(ctx, items))
}

/// `ops.aesKw(key, data, wrap) -> bytes`
fn op_aes_kw(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let key = bytes_arg(args.get_or_undefined(0), ctx)?;
    let data = bytes_arg(args.get_or_undefined(1), ctx)?;
    match aes_kw(&key, &data, args.get_or_undefined(2).to_boolean()) {
        Ok(out) => bytes_to_js(&out, ctx),
        Err(e) => Err(e.throw(ctx)),
    }
}

pub(crate) const OPS: &[(&str, usize, OpFn)] = &[
    ("cipherNew", 5, op_cipher_new),
    ("cipherUpdate", 2, op_cipher_update),
    ("cipherFinal", 1, op_cipher_final),
    ("cipherCtl", 3, op_cipher_ctl),
    ("cipherInfo", 1, op_cipher_info),
    ("ciphers", 0, op_ciphers),
    ("aesKw", 3, op_aes_kw),
];

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(b: &[u8]) -> String {
        b.iter().map(|x| format!("{x:02x}")).collect()
    }
    fn unhex(s: &str) -> Vec<u8> {
        (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
    }

    #[test]
    fn matches_node_vectors() {
        let key = b"0123456789abcdef";
        let iv = b"fedcba9876543210";
        // node -e 'c=require("crypto");x=c.createCipheriv(N,K,IV);console.log(Buffer.concat([x.update("hello world, this is a test"),x.final()]).toString("hex"))'
        assert_eq!(hex(&one_shot("aes-128-cbc", key, Some(iv), true, b"hello world, this is a test").unwrap()), "48a64c77eda317c2ece0b3eb1013593a9ddabef656d84916dd63e7bd9f8a76f4");
        assert_eq!(hex(&one_shot("aes-128-ctr", key, Some(iv), true, b"hello world, this is a test").unwrap()), "632eba1d99500866ca54a7faea6802543145854a8fc6adfb838b25");
        let ct = one_shot("aes-128-cbc", key, Some(iv), true, b"x").unwrap();
        assert_eq!(one_shot("aes-128-cbc", key, Some(iv), false, &ct).unwrap(), b"x");
    }

    #[test]
    fn gcm_matches_nist_vector() {
        // NIST GCM test case 4 (AES-128, 60-byte plaintext, 20-byte AAD).
        let key = unhex("feffe9928665731c6d6a8f9467308308");
        let iv = unhex("cafebabefacedbaddecaf888");
        let pt = unhex("d9313225f88406e5a55909c5aff5269a86a7a9531534f7da2e4c303d8a318a721c3c0c95956809532fcf0e2449a6b525b16aedf5aa0de657ba637b39");
        let aad = unhex("feedfacedeadbeeffeedfacedeadbeefabaddad2");
        let mut c = CipherState::new("aes-128-gcm", &key, Some(&iv), true, None).unwrap();
        c.set_aad(&aad).unwrap();
        let mut ct = Vec::new();
        for chunk in pt.chunks(7) {
            ct.extend(c.update(chunk).unwrap());
        }
        ct.extend(c.finalize().unwrap());
        assert_eq!(hex(&ct), "42831ec2217774244b7221b784d0d49ce3aa212f2c02a4e035c17e2329aca12e21d514b25466931c7d8f6a5aac84aa051ba30b396a0aac973d58e091");
        assert_eq!(hex(&c.auth_tag().unwrap()), "5bc94fbc3221a5db94fae95ae7121a47");
        let mut d = CipherState::new("aes-128-gcm", &key, Some(&iv), false, None).unwrap();
        d.set_aad(&aad).unwrap();
        d.set_auth_tag(&c.auth_tag().unwrap()).unwrap();
        assert_eq!(d.update(&ct).unwrap(), pt);
        d.finalize().unwrap();
        let mut bad = CipherState::new("aes-128-gcm", &key, Some(&iv), false, None).unwrap();
        bad.set_auth_tag(&[0; 16]).unwrap();
        bad.update(&ct).unwrap();
        assert_eq!(bad.finalize().unwrap_err().message, "Unsupported state or unable to authenticate data");
    }

    #[test]
    fn chacha20_poly1305_matches_rfc8439() {
        let key = unhex("808182838485868788898a8b8c8d8e8f909192939495969798999a9b9c9d9e9f");
        let iv = unhex("070000004041424344454647");
        let aad = unhex("50515253c0c1c2c3c4c5c6c7");
        let pt = b"Ladies and Gentlemen of the class of '99: If I could offer you only one tip for the future, sunscreen would be it.";
        let mut c = CipherState::new("chacha20-poly1305", &key, Some(&iv), true, None).unwrap();
        c.set_aad(&aad).unwrap();
        let ct = c.update(pt).unwrap();
        c.finalize().unwrap();
        assert_eq!(hex(&ct[..16]), "d31a8d34648e60db7b86afbc53ef7ec2");
        assert_eq!(hex(&c.auth_tag().unwrap()), "1ae10b594f09e26a7e902ecbd0600691");
    }

    #[test]
    fn errors_match_node() {
        let e = CipherState::new("foo", &[0; 16], Some(&[0; 16]), true, None).err().unwrap();
        assert_eq!((e.message.as_str(), e.code), ("Unknown cipher", Some("ERR_CRYPTO_UNKNOWN_CIPHER")));
        let e = CipherState::new("aes-128-cbc", &[0; 15], Some(&[0; 16]), true, None).err().unwrap();
        assert_eq!((e.kind, e.message.as_str()), ("RangeError", "Invalid key length"));
        let e = CipherState::new("aes-128-cbc", &[0; 16], Some(&[0; 15]), true, None).err().unwrap();
        assert_eq!((e.kind, e.message.as_str()), ("TypeError", "Invalid initialization vector"));
        let e = one_shot("aes-128-cbc", &[0; 16], Some(&[0; 16]), false, &[0; 16]).unwrap_err();
        assert_eq!(e.code, Some("ERR_OSSL_BAD_DECRYPT"));
        let e = one_shot("aes-128-cbc", &[0; 16], Some(&[0; 16]), false, &[0; 15]).unwrap_err();
        assert_eq!(e.code, Some("ERR_OSSL_WRONG_FINAL_BLOCK_LENGTH"));
    }

    #[test]
    fn aes_kw_rfc3394() {
        let kek = unhex("000102030405060708090A0B0C0D0E0F");
        let data = unhex("00112233445566778899AABBCCDDEEFF");
        let wrapped = aes_kw(&kek, &data, true).unwrap();
        assert_eq!(hex(&wrapped), "1fa68b0a8112b447aef34bd8fb5a7b829d3e862371d2cfe5");
        assert_eq!(aes_kw(&kek, &wrapped, false).unwrap(), data);
    }
}
