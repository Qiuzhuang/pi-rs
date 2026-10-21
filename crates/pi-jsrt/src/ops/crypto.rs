//! Hashing and randomness ops (`crypto.createHash`, `randomUUID`, `getRandomValues`).

use boa_engine::object::builtins::{JsArrayBuffer, JsTypedArray};
use boa_engine::{Context, JsArgs, JsResult, JsValue};
use rand::RngCore;
use sha2::Digest;

use super::{bytes_arg, bytes_to_js};
use crate::engine::{OpFn, arg_f64, arg_string, js_str, type_error};

/// Hash `data` with a Node algorithm name. Returns `None` for unknown algorithms.
pub(crate) fn hash(alg: &str, data: &[u8]) -> Option<Vec<u8>> {
    Some(match alg.to_ascii_lowercase().replace('-', "").as_str() {
        "sha256" => sha2::Sha256::digest(data).to_vec(),
        "sha224" => sha2::Sha224::digest(data).to_vec(),
        "sha384" => sha2::Sha384::digest(data).to_vec(),
        "sha512" => sha2::Sha512::digest(data).to_vec(),
        "sha1" => sha1::Sha1::digest(data).to_vec(),
        "md5" => md5::Md5::digest(data).to_vec(),
        _ => return None,
    })
}

/// Block size in bytes for HMAC.
fn block_size(alg: &str) -> Option<usize> {
    Some(match alg.to_ascii_lowercase().replace('-', "").as_str() {
        "sha256" | "sha224" | "sha1" | "md5" => 64,
        "sha384" | "sha512" => 128,
        _ => return None,
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
