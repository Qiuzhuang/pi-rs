//! Hashing, HMAC, randomness and key derivation ops (`crypto.createHash`, `randomUUID`,
//! `getRandomValues`, `pbkdf2`, `scrypt`, `hkdf`).

use boa_engine::object::builtins::{JsArrayBuffer, JsTypedArray};
use boa_engine::{Context, JsArgs, JsResult, JsValue};
use rand::RngCore;
use sha2::Digest;

use super::{bytes_arg, bytes_to_js};
use crate::engine::{OpFn, arg_f64, arg_string, js_str, type_error};

/// Canonical digest name for a Node/OpenSSL digest spelling (`SHA256`, `RSA-SHA256`,
/// `sha256WithRSAEncryption`, `SHA-256`, `sha512-256`, ...).
pub(crate) fn digest_name(alg: &str) -> Option<&'static str> {
    let mut a = alg.to_ascii_lowercase();
    for prefix in ["rsa-", "ecdsa-with-"] {
        if let Some(rest) = a.strip_prefix(prefix) {
            a = rest.to_string();
        }
    }
    if let Some(rest) = a.strip_suffix("withrsaencryption") {
        a = rest.to_string();
    }
    Some(match a.replace(['-', '/', '_'], "").as_str() {
        "md5" => "md5",
        "sha1" => "sha1",
        "sha224" => "sha224",
        "sha256" => "sha256",
        "sha384" => "sha384",
        "sha512" => "sha512",
        "sha512224" => "sha512-224",
        "sha512256" => "sha512-256",
        _ => return None,
    })
}

/// Run `$body` with `$D` bound to the digest type named by the canonical `$name`.
macro_rules! with_digest {
    ($name:expr, $D:ident => $body:expr) => {
        match $name {
            "md5" => {
                type $D = md5::Md5;
                Some($body)
            }
            "sha1" => {
                type $D = sha1::Sha1;
                Some($body)
            }
            "sha224" => {
                type $D = sha2::Sha224;
                Some($body)
            }
            "sha256" => {
                type $D = sha2::Sha256;
                Some($body)
            }
            "sha384" => {
                type $D = sha2::Sha384;
                Some($body)
            }
            "sha512" => {
                type $D = sha2::Sha512;
                Some($body)
            }
            "sha512-224" => {
                type $D = sha2::Sha512_224;
                Some($body)
            }
            "sha512-256" => {
                type $D = sha2::Sha512_256;
                Some($body)
            }
            _ => None,
        }
    };
}
#[allow(unused_imports)]
pub(crate) use with_digest;

/// Hash `data` with a Node algorithm name. Returns `None` for unknown algorithms.
pub(crate) fn hash(alg: &str, data: &[u8]) -> Option<Vec<u8>> {
    let name = digest_name(alg)?;
    with_digest!(name, D => D::digest(data).to_vec())
}

/// Block size in bytes for HMAC.
fn block_size(alg: &str) -> Option<usize> {
    Some(match digest_name(alg)? {
        "sha384" | "sha512" | "sha512-224" | "sha512-256" => 128,
        _ => 64,
    })
}

fn op_hash(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let alg = arg_string(args, 0, ctx)?;
    let data = bytes_arg(args.get_or_undefined(1), ctx)?;
    match hash(&alg, &data) {
        Some(d) => bytes_to_js(&d, ctx),
        None => Err(boa_engine::JsNativeError::error().with_message("Digest method not supported").into()),
    }
}

fn op_hmac(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let alg = arg_string(args, 0, ctx)?;
    let mut key = bytes_arg(args.get_or_undefined(1), ctx)?;
    let data = bytes_arg(args.get_or_undefined(2), ctx)?;
    let (Some(bs), Some(_)) = (block_size(&alg), hash(&alg, b"")) else {
        return Err(boa_engine::JsNativeError::error().with_message("Invalid digest").into());
    };
    if key.len() > bs {
        key = hash(&alg, &key).unwrap_or_default();
    }
    key.resize(bs, 0);
    let mut inner: Vec<u8> = key.iter().map(|b| b ^ 0x36).collect();
    inner.extend_from_slice(&data);
    let inner_hash = hash(&alg, &inner).unwrap_or_default();
    let mut outer: Vec<u8> = key.iter().map(|b| b ^ 0x5c).collect();
    outer.extend_from_slice(&inner_hash);
    bytes_to_js(&hash(&alg, &outer).unwrap_or_default(), ctx)
}

fn op_random_uuid(_: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    Ok(js_str(&uuid::Uuid::new_v4().to_string()))
}

fn op_random_bytes(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let n = arg_f64(args, 0, ctx)?;
    if !(0.0..=(1u64 << 31) as f64).contains(&n) {
        return Err(boa_engine::JsNativeError::range().with_message("The value of \"size\" is out of range.").into());
    }
    let mut buf = vec![0u8; n as usize];
    rand::thread_rng().fill_bytes(&mut buf);
    bytes_to_js(&buf, ctx)
}

fn op_fill_random(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let v = args.get_or_undefined(0).clone();
    let obj = v.as_object().ok_or_else(|| type_error("The data argument must be an integer-type TypedArray"))?;
    let ta = JsTypedArray::from_object(obj).map_err(|_| type_error("The data argument must be an integer-type TypedArray"))?;
    let offset = ta.byte_offset(ctx)?;
    let len = ta.byte_length(ctx)?;
    if len > 65536 {
        return Err(boa_engine::JsNativeError::error()
            .with_message(format!(
                "The ArrayBufferView's byte length ({len}) exceeds the number of bytes of entropy available via this API (65536)"
            ))
            .into());
    }
    let buffer = ta.buffer(ctx)?;
    if let Some(b) = buffer.as_object()
        && let Ok(ab) = JsArrayBuffer::from_object(b)
        && let Some(mut data) = ab.data_mut()
    {
        rand::thread_rng().fill_bytes(&mut data[offset..offset + len]);
    }
    Ok(v)
}

pub(crate) const WEB_OPS: &[(&str, usize, OpFn)] = &[
    ("hash", 2, op_hash),
    ("hmac", 3, op_hmac),
    ("randomUUID", 0, op_random_uuid),
    ("fillRandom", 1, op_fill_random),
    ("randomBytes", 1, op_random_bytes),
];

pub(crate) const OPS: &[(&str, usize, OpFn)] = &[];

// ---------------------------------------------------------------------------
// Key derivation (pbkdf2, scrypt, hkdf)
// ---------------------------------------------------------------------------

use super::cipher::CipherError;

fn invalid_digest(alg: &str) -> CipherError {
    CipherError::new("TypeError", &format!("Invalid digest: {alg}"), Some("ERR_CRYPTO_INVALID_DIGEST"))
}

pub(crate) fn pbkdf2(password: &[u8], salt: &[u8], iterations: u32, keylen: usize, digest: &str) -> Result<Vec<u8>, CipherError> {
    let name = digest_name(digest).ok_or_else(|| invalid_digest(digest))?;
    let mut out = vec![0u8; keylen];
    with_digest!(name, D => pbkdf2::pbkdf2_hmac::<D>(password, salt, iterations, &mut out)).ok_or_else(|| invalid_digest(digest))?;
    Ok(out)
}

pub(crate) fn scrypt(password: &[u8], salt: &[u8], keylen: usize, n: u64, r: u32, p: u32, maxmem: u64) -> Result<Vec<u8>, CipherError> {
    let invalid = |why: &str| CipherError::new("RangeError", &format!("Invalid scrypt params: {why}"), Some("ERR_CRYPTO_INVALID_SCRYPT_PARAMS"));
    if n < 2 || !n.is_power_of_two() {
        return Err(invalid("error:030000AC:digital envelope routines::invalid N"));
    }
    if 128 * n * r as u64 > maxmem {
        return Err(invalid("error:030000AC:digital envelope routines::memory limit exceeded"));
    }
    let params = scrypt::Params::new(n.trailing_zeros() as u8, r, p, keylen.max(10)).map_err(|e| invalid(&e.to_string()))?;
    let mut out = vec![0u8; keylen];
    scrypt::scrypt(password, salt, &params, &mut out).map_err(|e| invalid(&e.to_string()))?;
    Ok(out)
}

pub(crate) fn hkdf(digest: &str, ikm: &[u8], salt: &[u8], info: &[u8], keylen: usize) -> Result<Vec<u8>, CipherError> {
    let name = digest_name(digest).ok_or_else(|| invalid_digest(digest))?;
    let mut out = vec![0u8; keylen];
    let ok = with_digest!(name, D => hkdf::Hkdf::<D>::new(Some(salt), ikm).expand(info, &mut out).is_ok()).ok_or_else(|| invalid_digest(digest))?;
    if !ok {
        return Err(CipherError::new("RangeError", "Invalid key length", Some("ERR_CRYPTO_INVALID_KEYLEN")));
    }
    Ok(out)
}

pub(crate) fn to_error(e: CipherError) -> crate::Error {
    crate::Error::Js(crate::error::JsException { name: e.kind.into(), message: e.message, stack: None, code: e.code.map(Into::into) })
}

/// Run a crypto job: synchronously, or on tokio's blocking pool when `asynchronous` (returns a
/// promise of the bytes).
pub(crate) fn job(ctx: &mut Context, asynchronous: bool, f: impl FnOnce() -> Result<Vec<u8>, CipherError> + Send + 'static) -> JsResult<JsValue> {
    if !asynchronous {
        return match f() {
            Ok(b) => bytes_to_js(&b, ctx),
            Err(e) => Err(e.throw(ctx)),
        };
    }
    Ok(crate::engine::spawn_result(
        ctx,
        async move {
            match tokio::task::spawn_blocking(f).await {
                Ok(r) => r.map_err(to_error),
                Err(e) => Err(crate::Error::Host(e.to_string())),
            }
        },
        |b, ctx| bytes_to_js(&b, ctx),
    ))
}

/// `ops.pbkdf2(password, salt, iterations, keylen, digest, async)`
fn op_pbkdf2(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let password = bytes_arg(args.get_or_undefined(0), ctx)?;
    let salt = bytes_arg(args.get_or_undefined(1), ctx)?;
    let iterations = arg_f64(args, 2, ctx)? as u32;
    let keylen = arg_f64(args, 3, ctx)? as usize;
    let digest = arg_string(args, 4, ctx)?;
    let asynchronous = args.get_or_undefined(5).to_boolean();
    job(ctx, asynchronous, move || pbkdf2(&password, &salt, iterations, keylen, &digest))
}

/// `ops.scrypt(password, salt, keylen, N, r, p, maxmem, async)`
fn op_scrypt(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let password = bytes_arg(args.get_or_undefined(0), ctx)?;
    let salt = bytes_arg(args.get_or_undefined(1), ctx)?;
    let keylen = arg_f64(args, 2, ctx)? as usize;
    let n = arg_f64(args, 3, ctx)? as u64;
    let r = arg_f64(args, 4, ctx)? as u32;
    let p = arg_f64(args, 5, ctx)? as u32;
    let maxmem = arg_f64(args, 6, ctx)? as u64;
    let asynchronous = args.get_or_undefined(7).to_boolean();
    job(ctx, asynchronous, move || scrypt(&password, &salt, keylen, n, r, p, maxmem))
}

/// `ops.hkdf(digest, ikm, salt, info, keylen, async)`
fn op_hkdf(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let digest = arg_string(args, 0, ctx)?;
    let ikm = bytes_arg(args.get_or_undefined(1), ctx)?;
    let salt = bytes_arg(args.get_or_undefined(2), ctx)?;
    let info = bytes_arg(args.get_or_undefined(3), ctx)?;
    let keylen = arg_f64(args, 4, ctx)? as usize;
    let asynchronous = args.get_or_undefined(5).to_boolean();
    job(ctx, asynchronous, move || hkdf(&digest, &ikm, &salt, &info, keylen))
}

/// `ops.digestInfo(name) -> canonical name | undefined`
fn op_digest_name(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let name = arg_string(args, 0, ctx)?;
    Ok(digest_name(&name).map(js_str).unwrap_or_default())
}

pub(crate) const KDF_OPS: &[(&str, usize, OpFn)] = &[
    ("pbkdf2", 6, op_pbkdf2),
    ("scrypt", 8, op_scrypt),
    ("hkdf", 6, op_hkdf),
    ("digestName", 1, op_digest_name),
];

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(b: &[u8]) -> String {
        b.iter().map(|x| format!("{x:02x}")).collect()
    }

    #[test]
    fn kdfs_match_node() {
        assert_eq!(hex(&pbkdf2(b"pw", b"salt", 1000, 16, "sha256").unwrap()), "0a38253555ce37f5c72a6b703f996814");
        assert_eq!(hex(&scrypt(b"pw", b"salt", 16, 16384, 8, 1, 32 << 20).unwrap()), "c0b515908e61334cae6d6003c00be60e");
        assert_eq!(hex(&hkdf("sha256", b"key", b"salt", b"info", 16).unwrap()), "9ca0d662557439e3b83365f2da4626d3");
        assert_eq!(pbkdf2(b"a", b"b", 1, 32, "foo").unwrap_err().message, "Invalid digest: foo");
        assert!(scrypt(b"a", b"b", 64, 1 << 20, 8, 1, 32 << 20).unwrap_err().message.contains("memory limit exceeded"));
        assert_eq!(digest_name("RSA-SHA256"), Some("sha256"));
        assert_eq!(digest_name("sha256WithRSAEncryption"), Some("sha256"));
        assert_eq!(digest_name("SHA-512/256"), Some("sha512-256"));
    }
}
