//! Compression ops backing `node:zlib`: deflate/inflate (zlib, raw, gzip, auto-detect) on
//! miniz_oxide, brotli on the `brotli` crate, zstd on ruzstd, and CRC-32.
//!
//! A stream is an opaque JS object owning a [`ZStream`]. `ops.zlibWrite(stream, input, flush)`
//! consumes all of `input` and returns everything produced so far (the JS glue slices it for
//! callers that pass bounded output buffers, like Node's internal handle API used by ssh2).
//! Errors are thrown as `Error`s with Node's zlib `message`, `code` and `errno`.

use std::io::Read;

use boa_engine::{Context, JsArgs, JsResult, JsValue};
use miniz_oxide::deflate::core::{CompressorOxide, create_comp_flags_from_zip_params};
use miniz_oxide::inflate::TINFLStatus;
use miniz_oxide::inflate::stream::InflateState;
use miniz_oxide::{DataFormat, MZError, MZFlush, MZStatus};

use super::{bytes_arg, bytes_to_js, coded_error, native_object, with_native};
use crate::engine::{OpFn, arg_f64, js_str};

// Node's mode numbers (`zlib.constants`).
const DEFLATE: i32 = 1;
const INFLATE: i32 = 2;
const GZIP: i32 = 3;
const GUNZIP: i32 = 4;
const DEFLATERAW: i32 = 5;
const INFLATERAW: i32 = 6;
#[cfg_attr(not(test), allow(dead_code))]
const UNZIP: i32 = 7;
const BROTLI_DECODE: i32 = 8;
const BROTLI_ENCODE: i32 = 9;
const ZSTD_COMPRESS: i32 = 10;
const ZSTD_DECOMPRESS: i32 = 11;

// Flush values.
const Z_NO_FLUSH: i32 = 0;
const Z_PARTIAL_FLUSH: i32 = 1;
const Z_SYNC_FLUSH: i32 = 2;
const Z_FULL_FLUSH: i32 = 3;
const Z_FINISH: i32 = 4;
const Z_BLOCK: i32 = 5;

const CHUNK: usize = 16 * 1024;

/// A zlib error: Node's message, code and errno.
#[derive(Debug, Clone)]
pub(crate) struct ZError {
    pub message: String,
    pub code: String,
    pub errno: i32,
}

impl ZError {
    fn data(message: &str) -> Self {
        Self { message: message.into(), code: "Z_DATA_ERROR".into(), errno: -3 }
    }
    fn eof() -> Self {
        Self { message: "unexpected end of file".into(), code: "Z_BUF_ERROR".into(), errno: -5 }
    }
}

type ZResult<T> = Result<T, ZError>;

// ---------------------------------------------------------------------------
// Deflate
// ---------------------------------------------------------------------------

struct Deflater {
    comp: Box<CompressorOxide>,
    gzip: bool,
    level: i32,
    header_written: bool,
    crc: crc32fast::Hasher,
    size: u32,
    finished: bool,
}

impl Deflater {
    fn new(mode: i32, level: i32, strategy: i32) -> Self {
        let level = if level < 0 { 6 } else { level.min(9) };
        let window_bits = if mode == DEFLATE { 15 } else { -15 };
        let flags = create_comp_flags_from_zip_params(level, window_bits, strategy);
        Self {
            comp: Box::new(CompressorOxide::new(flags)),
            gzip: mode == GZIP,
            level,
            header_written: false,
            crc: crc32fast::Hasher::new(),
            size: 0,
            finished: false,
        }
    }

    fn write(&mut self, input: &[u8], flush: i32, out: &mut Vec<u8>) -> ZResult<()> {
        if self.finished {
            return Ok(());
        }
        if self.gzip && !self.header_written {
            // Like zlib: no mtime, the extra flags hint at the level, OS = Unix.
            let xfl = match self.level {
                9 => 2,
                1 => 4,
                _ => 0,
            };
            out.extend_from_slice(&[0x1f, 0x8b, 8, 0, 0, 0, 0, 0, xfl, 3]);
            self.header_written = true;
        }
        if self.gzip {
            self.crc.update(input);
            self.size = self.size.wrapping_add(input.len() as u32);
        }
        let mz_flush = match flush {
            Z_NO_FLUSH | Z_BLOCK => MZFlush::None,
            // miniz_oxide's partial flush emits nothing; a sync flush also makes everything
            // written so far decodable (it ends with an empty stored block instead).
            Z_PARTIAL_FLUSH | Z_SYNC_FLUSH => MZFlush::Sync,
            Z_FULL_FLUSH => MZFlush::Full,
            _ => MZFlush::Finish,
        };
        let mut pos = 0;
        loop {
            let start = out.len();
            let room = CHUNK.max(input.len() - pos + 64);
            out.resize(start + room, 0);
            let r = miniz_oxide::deflate::stream::deflate(&mut self.comp, &input[pos..], &mut out[start..], mz_flush);
            out.truncate(start + r.bytes_written);
            pos += r.bytes_consumed;
            match r.status {
                Ok(MZStatus::StreamEnd) => {
                    self.finished = true;
                    break;
                }
                Ok(_) | Err(MZError::Buf) => {
                    if pos >= input.len() && r.bytes_written < room {
                        break;
                    }
                    if r.bytes_written == 0 && r.bytes_consumed == 0 {
                        break;
                    }
                }
                Err(_) => return Err(ZError { message: "stream error".into(), code: "Z_STREAM_ERROR".into(), errno: -2 }),
            }
        }
        if self.finished && self.gzip {
            let crc = std::mem::take(&mut self.crc).finalize();
            out.extend_from_slice(&crc.to_le_bytes());
            out.extend_from_slice(&self.size.to_le_bytes());
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Inflate
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq)]
enum InKind {
    Zlib,
    Raw,
    Gzip,
    Auto,
}

enum Phase {
    /// UNZIP: waiting for the first byte to pick gzip or zlib.
    Detect,
    /// Collecting a zlib header (2 bytes) to validate it.
    ZlibHeader,
    /// Collecting a gzip member header.
    GzHeader,
    Body,
    GzTrailer,
    /// Between gzip members.
    AfterMember,
    Done,
}

struct Inflater {
    kind: InKind,
    phase: Phase,
    pending: Vec<u8>,
    state: Box<InflateState>,
    crc: crc32fast::Hasher,
    size: u32,
    /// At least one gzip member was decoded (later members are optional).
    member_done: bool,
}

impl Inflater {
    fn new(mode: i32) -> Self {
        let kind = match mode {
            INFLATE => InKind::Zlib,
            INFLATERAW => InKind::Raw,
            GUNZIP => InKind::Gzip,
            _ => InKind::Auto,
        };
        let (phase, format) = match kind {
            InKind::Zlib => (Phase::ZlibHeader, DataFormat::Zlib),
            InKind::Raw => (Phase::Body, DataFormat::Raw),
            InKind::Gzip => (Phase::GzHeader, DataFormat::Raw),
            InKind::Auto => (Phase::Detect, DataFormat::Raw),
        };
        Self {
            kind,
            phase,
            pending: Vec::new(),
            state: InflateState::new_boxed(format),
            crc: crc32fast::Hasher::new(),
            size: 0,
            member_done: false,
        }
    }

    fn ended(&self) -> bool {
        matches!(self.phase, Phase::Done) || (matches!(self.phase, Phase::AfterMember) && self.member_done)
    }

    fn write(&mut self, input: &[u8], flush: i32, out: &mut Vec<u8>) -> ZResult<()> {
        let data: Vec<u8>;
        let mut input = input;
        if !self.pending.is_empty() {
            let mut joined = std::mem::take(&mut self.pending);
            joined.extend_from_slice(input);
            data = joined;
            input = &data;
        }
        let mut pos = 0;
        loop {
            match self.phase {
                Phase::Done => return Ok(()),
                Phase::Detect => {
                    if pos >= input.len() {
                        break;
                    }
                    if input[pos] == 0x1f {
                        self.kind = InKind::Gzip;
                        self.phase = Phase::GzHeader;
                    } else {
                        self.kind = InKind::Zlib;
                        self.phase = Phase::ZlibHeader;
                        self.state.reset(DataFormat::Zlib);
                    }
                }
                Phase::ZlibHeader => {
                    let rest = &input[pos..];
                    if rest.len() < 2 {
                        if !rest.is_empty() && (rest[0] & 0x0f != 8 || rest[0] >> 4 > 7) {
                            return Err(ZError::data("incorrect header check"));
                        }
                        self.pending = rest.to_vec();
                        pos = input.len();
                        break;
                    }
                    let (cmf, flg) = (rest[0] as u16, rest[1] as u16);
                    if cmf & 0x0f != 8 || (cmf << 8 | flg) % 31 != 0 {
                        return Err(ZError::data("incorrect header check"));
                    }
                    if cmf >> 4 > 7 {
                        return Err(ZError::data("invalid window size"));
                    }
                    if flg & 0x20 != 0 {
                        return Err(ZError { message: "Missing dictionary".into(), code: "Z_NEED_DICT".into(), errno: 2 });
                    }
                    self.phase = Phase::Body;
                }
                Phase::GzHeader => {
                    let rest = &input[pos..];
                    match parse_gzip_header(rest)? {
                        Some(len) => {
                            pos += len;
                            self.state.reset(DataFormat::Raw);
                            self.crc = crc32fast::Hasher::new();
                            self.size = 0;
                            self.phase = Phase::Body;
                        }
                        None => {
                            self.pending = rest.to_vec();
                            pos = input.len();
                            break;
                        }
                    }
                }
                Phase::Body => {
                    let (consumed, ended) = self.inflate(&input[pos..], out)?;
                    pos += consumed;
                    if !ended {
                        break;
                    }
                    self.phase = if self.kind == InKind::Gzip { Phase::GzTrailer } else { Phase::Done };
                }
                Phase::GzTrailer => {
                    let rest = &input[pos..];
                    if rest.len() < 8 {
                        self.pending = rest.to_vec();
                        pos = input.len();
                        break;
                    }
                    let crc = u32::from_le_bytes([rest[0], rest[1], rest[2], rest[3]]);
                    let size = u32::from_le_bytes([rest[4], rest[5], rest[6], rest[7]]);
                    if crc != std::mem::take(&mut self.crc).finalize() {
                        return Err(ZError::data("incorrect data check"));
                    }
                    if size != self.size {
                        return Err(ZError::data("incorrect length check"));
                    }
                    pos += 8;
                    self.member_done = true;
                    self.phase = Phase::AfterMember;
                }
                Phase::AfterMember => {
                    if pos >= input.len() {
                        break;
                    }
                    // Like Node: zero padding after a member ends the stream, anything else must
                    // be another member.
                    if input[pos] == 0 {
                        self.phase = Phase::Done;
                        return Ok(());
                    }
                    self.phase = Phase::GzHeader;
                }
            }
        }
        debug_assert!(pos <= input.len());
        if flush == Z_FINISH && !self.ended() {
            return Err(ZError::eof());
        }
        Ok(())
    }

    /// Feed body bytes; returns (consumed, stream ended).
    fn inflate(&mut self, input: &[u8], out: &mut Vec<u8>) -> ZResult<(usize, bool)> {
        let mut pos = 0;
        loop {
            let start = out.len();
            out.resize(start + CHUNK, 0);
            let r = miniz_oxide::inflate::stream::inflate(&mut self.state, &input[pos..], &mut out[start..], MZFlush::None);
            if self.kind == InKind::Gzip {
                self.crc.update(&out[start..start + r.bytes_written]);
                self.size = self.size.wrapping_add(r.bytes_written as u32);
            }
            out.truncate(start + r.bytes_written);
            pos += r.bytes_consumed;
            match r.status {
                Ok(MZStatus::StreamEnd) => return Ok((pos, true)),
                Ok(_) | Err(MZError::Buf) => {
                    if r.bytes_written < CHUNK && (pos >= input.len() || (r.bytes_written == 0 && r.bytes_consumed == 0)) {
                        return Ok((pos, false));
                    }
                }
                Err(_) => {
                    return Err(match self.state.last_status() {
                        TINFLStatus::Adler32Mismatch => ZError::data("incorrect data check"),
                        _ => ZError::data("invalid block type"),
                    });
                }
            }
        }
    }
}

/// Parse a gzip member header: `Ok(Some(len))` when complete, `Ok(None)` when more bytes are
/// needed.
fn parse_gzip_header(b: &[u8]) -> ZResult<Option<usize>> {
    if !b.is_empty() && b[0] != 0x1f || b.len() > 1 && b[1] != 0x8b {
        return Err(ZError::data("incorrect header check"));
    }
    if b.len() > 2 && b[2] != 8 {
        return Err(ZError::data("unknown compression method"));
    }
    if b.len() < 10 {
        return Ok(None);
    }
    let flags = b[3];
    if flags & 0xe0 != 0 {
        return Err(ZError::data("unknown header flags set"));
    }
    let mut pos = 10;
    if flags & 4 != 0 {
        if b.len() < pos + 2 {
            return Ok(None);
        }
        let xlen = u16::from_le_bytes([b[pos], b[pos + 1]]) as usize;
        pos += 2 + xlen;
        if b.len() < pos {
            return Ok(None);
        }
    }
    for flag in [8u8, 16] {
        if flags & flag != 0 {
            match b[pos..].iter().position(|&c| c == 0) {
                Some(i) => pos += i + 1,
                None => return Ok(None),
            }
        }
    }
    if flags & 2 != 0 {
        pos += 2;
        if b.len() < pos {
            return Ok(None);
        }
    }
    Ok(Some(pos))
}

// ---------------------------------------------------------------------------
// Brotli
// ---------------------------------------------------------------------------

type BrotliDecoder = brotli::BrotliState<brotli::HeapAlloc<u8>, brotli::HeapAlloc<u32>, brotli::HeapAlloc<brotli::HuffmanCode>>;
type BrotliEncoder = brotli::enc::encode::BrotliEncoderStateStruct<brotli::enc::StandardAlloc>;

fn brotli_encoder(params: &[(u32, u32)]) -> BrotliEncoder {
    use brotli::enc::encode::BrotliEncoderParameter as P;
    let mut enc = BrotliEncoder::new(brotli::enc::StandardAlloc::default());
    enc.set_parameter(P::BROTLI_PARAM_QUALITY, 11);
    enc.set_parameter(P::BROTLI_PARAM_LGWIN, 22);
    for &(key, value) in params {
        let p = match key {
            0 => P::BROTLI_PARAM_MODE,
            1 => P::BROTLI_PARAM_QUALITY,
            2 => P::BROTLI_PARAM_LGWIN,
            3 => P::BROTLI_PARAM_LGBLOCK,
            4 => P::BROTLI_PARAM_DISABLE_LITERAL_CONTEXT_MODELING,
            5 => P::BROTLI_PARAM_SIZE_HINT,
            6 => P::BROTLI_PARAM_LARGE_WINDOW,
            _ => continue,
        };
        enc.set_parameter(p, value);
    }
    enc
}

fn brotli_encode(enc: &mut BrotliEncoder, input: &[u8], flush: i32, out: &mut Vec<u8>) -> ZResult<()> {
    use brotli::enc::encode::BrotliEncoderOperation as Op;
    let op = match flush {
        1 => Op::BROTLI_OPERATION_FLUSH,
        2 => Op::BROTLI_OPERATION_FINISH,
        _ => Op::BROTLI_OPERATION_PROCESS,
    };
    if enc.is_finished() {
        return Ok(());
    }
    let mut avail_in = input.len();
    let mut in_off = 0;
    let mut buf = vec![0u8; CHUNK];
    loop {
        let mut avail_out = buf.len();
        let mut out_off = 0;
        let ok = enc.compress_stream(op, &mut avail_in, input, &mut in_off, &mut avail_out, &mut buf, &mut out_off, &mut None, &mut |_, _, _, _| ());
        if !ok {
            return Err(ZError { message: "Compression failed".into(), code: "ERR_BROTLI_COMPRESSION_FAILED".into(), errno: -1 });
        }
        out.extend_from_slice(&buf[..out_off]);
        let done = match op {
            Op::BROTLI_OPERATION_FINISH => enc.is_finished(),
            _ => avail_in == 0 && !enc.has_more_output(),
        };
        if done || (out_off == 0 && avail_in == 0 && !enc.has_more_output() && op == Op::BROTLI_OPERATION_PROCESS) {
            return Ok(());
        }
    }
}

struct BrotliDec {
    state: Box<BrotliDecoder>,
    done: bool,
}

fn brotli_decode(dec: &mut BrotliDec, input: &[u8], flush: i32, out: &mut Vec<u8>) -> ZResult<()> {
    if dec.done {
        return Ok(());
    }
    let mut avail_in = input.len();
    let mut in_off = 0;
    let mut buf = vec![0u8; CHUNK];
    let mut total = 0usize;
    loop {
        let mut avail_out = buf.len();
        let mut out_off = 0;
        let r = brotli::BrotliDecompressStream(&mut avail_in, &mut in_off, input, &mut avail_out, &mut out_off, &mut buf, &mut total, &mut dec.state);
        out.extend_from_slice(&buf[..out_off]);
        match r {
            brotli::BrotliResult::ResultSuccess => {
                dec.done = true;
                return Ok(());
            }
            brotli::BrotliResult::NeedsMoreOutput => continue,
            brotli::BrotliResult::NeedsMoreInput => {
                // Node's brotli FINISH on a truncated stream reports the zlib EOF error.
                if flush == 2 {
                    return Err(ZError::eof());
                }
                return Ok(());
            }
            brotli::BrotliResult::ResultFailure => {
                let code = dec.state.error_code;
                let name = format!("{code:?}");
                let short = name.strip_prefix("BROTLI_DECODER").unwrap_or(&name);
                return Err(ZError { message: "Decompression failed".into(), code: format!("ERR_{short}"), errno: code as i32 });
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Streams
// ---------------------------------------------------------------------------

enum Engine {
    Deflate(Deflater),
    Inflate(Inflater),
    BrotliEnc(Box<BrotliEncoder>),
    BrotliDec(BrotliDec),
    /// zstd (de)compresses the whole input at `finish` (ruzstd has no incremental API here).
    Zstd { compress: bool, input: Vec<u8>, done: bool },
}

pub(crate) struct ZStream {
    mode: i32,
    level: i32,
    strategy: i32,
    params: Vec<(u32, u32)>,
    engine: Engine,
}

impl ZStream {
    pub(crate) fn new(mode: i32, level: i32, strategy: i32, params: Vec<(u32, u32)>) -> Self {
        let engine = Self::engine(mode, level, strategy, &params);
        Self { mode, level, strategy, params, engine }
    }

    fn engine(mode: i32, level: i32, strategy: i32, params: &[(u32, u32)]) -> Engine {
        match mode {
            DEFLATE | GZIP | DEFLATERAW => Engine::Deflate(Deflater::new(mode, level, strategy)),
            BROTLI_ENCODE => Engine::BrotliEnc(Box::new(brotli_encoder(params))),
            BROTLI_DECODE => Engine::BrotliDec(BrotliDec {
                state: Box::new(BrotliDecoder::new(Default::default(), Default::default(), Default::default())),
                done: false,
            }),
            ZSTD_COMPRESS | ZSTD_DECOMPRESS => Engine::Zstd { compress: mode == ZSTD_COMPRESS, input: Vec::new(), done: false },
            _ => Engine::Inflate(Inflater::new(mode)),
        }
    }

    pub(crate) fn reset(&mut self) {
        self.engine = Self::engine(self.mode, self.level, self.strategy, &self.params);
    }

    /// Consume `input`, appending output. `flush` is a zlib flush value (brotli/zstd: an
    /// operation, 2 = finish).
    pub(crate) fn write(&mut self, input: &[u8], flush: i32) -> ZResult<Vec<u8>> {
        let mut out = Vec::new();
        match &mut self.engine {
            Engine::Deflate(d) => d.write(input, flush, &mut out)?,
            Engine::Inflate(i) => i.write(input, flush, &mut out)?,
            Engine::BrotliEnc(e) => brotli_encode(e, input, flush, &mut out)?,
            Engine::BrotliDec(d) => brotli_decode(d, input, flush, &mut out)?,
            Engine::Zstd { compress, input: buf, done } => {
                if !*done {
                    buf.extend_from_slice(input);
                    if flush == 2 {
                        *done = true;
                        let data = std::mem::take(buf);
                        if *compress {
                            out = ruzstd::encoding::compress_to_vec(&data[..], ruzstd::encoding::CompressionLevel::Fastest);
                        } else {
                            let mut src = &data[..];
                            let mut decoder = ruzstd::decoding::StreamingDecoder::new(&mut src).map_err(|e| ZError {
                                message: format!("Decompression failed: {e}"),
                                code: "ZSTD_error_corruption_detected".into(),
                                errno: -20,
                            })?;
                            decoder.read_to_end(&mut out).map_err(|e| ZError {
                                message: format!("Decompression failed: {e}"),
                                code: "ZSTD_error_corruption_detected".into(),
                                errno: -20,
                            })?;
                        }
                    }
                }
            }
        }
        Ok(out)
    }
}

/// One-shot helper (tests).
#[cfg(test)]
pub(crate) fn process(mode: i32, level: i32, data: &[u8]) -> ZResult<Vec<u8>> {
    let finish = if matches!(mode, BROTLI_DECODE | BROTLI_ENCODE | ZSTD_COMPRESS | ZSTD_DECOMPRESS) { 2 } else { Z_FINISH };
    ZStream::new(mode, level, 0, Vec::new()).write(data, finish)
}

fn throw(ctx: &mut Context, e: ZError) -> boa_engine::JsError {
    coded_error(ctx, "Error", &e.message, &[("code", js_str(&e.code)), ("errno", JsValue::from(e.errno))])
}

/// `ops.zlibNew(mode, level, strategy, brotliParams?: number[]) -> stream`
fn op_zlib_new(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let mode = arg_f64(args, 0, ctx)? as i32;
    let level = args.get_or_undefined(1).to_number(ctx).map(|n| if n.is_nan() { -1 } else { n as i32 })?;
    let strategy = args.get_or_undefined(2).to_number(ctx).map(|n| if n.is_nan() { 0 } else { n as i32 })?;
    let mut params = Vec::new();
    if let Some(obj) = args.get_or_undefined(3).as_object() {
        let len = obj.get(boa_engine::js_string!("length"), ctx)?.to_number(ctx)? as u32;
        let mut i = 0;
        while i + 1 < len {
            let k = obj.get(i, ctx)?.to_number(ctx)? as u32;
            let v = obj.get(i + 1, ctx)?.to_number(ctx)? as u32;
            params.push((k, v));
            i += 2;
        }
    }
    if !(1..=11).contains(&mode) {
        return Err(crate::engine::type_error(format!("Bad argument: mode {mode}")));
    }
    Ok(native_object(ZStream::new(mode, level, strategy, params)))
}

/// `ops.zlibWrite(stream, input, flush) -> Uint8Array`
fn op_zlib_write(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let input = bytes_arg(args.get_or_undefined(1), ctx)?;
    let flush = arg_f64(args, 2, ctx)? as i32;
    let result = with_native::<ZStream, _>(args.get_or_undefined(0), |s| s.write(&input, flush))?;
    match result {
        Ok(out) => bytes_to_js(&out, ctx),
        Err(e) => Err(throw(ctx, e)),
    }
}

fn op_zlib_reset(_: &JsValue, args: &[JsValue], _ctx: &mut Context) -> JsResult<JsValue> {
    with_native::<ZStream, _>(args.get_or_undefined(0), |s| s.reset())?;
    Ok(JsValue::undefined())
}

/// `ops.crc32(data, initial) -> number`
fn op_crc32(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let data = bytes_arg(args.get_or_undefined(0), ctx)?;
    let init = args.get_or_undefined(1).to_number(ctx)?;
    let init = if init.is_finite() { init as u32 } else { 0 };
    let mut h = crc32fast::Hasher::new_with_initial(init);
    h.update(&data);
    Ok(JsValue::from(h.finalize()))
}

pub(crate) const OPS: &[(&str, usize, OpFn)] = &[
    ("zlibNew", 4, op_zlib_new),
    ("zlibWrite", 3, op_zlib_write),
    ("zlibReset", 1, op_zlib_reset),
    ("crc32", 2, op_crc32),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deflate_round_trips_and_matches_zlib_bytes() {
        // Node: zlib.deflateSync("abc").toString("hex")
        assert_eq!(hex(&process(DEFLATE, -1, b"abc").unwrap()), "789c4b4c4a0600024d0127");
        assert_eq!(hex(&process(GZIP, -1, b"abc").unwrap()), "1f8b08000000000000034b4c4a0600c241243503000000");
        assert_eq!(hex(&process(GZIP, 9, b"abc").unwrap()), "1f8b08000000000002034b4c4a0600c241243503000000");
        let big: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8 ^ (i / 1000) as u8).collect();
        for (c, d) in [(DEFLATE, INFLATE), (GZIP, GUNZIP), (DEFLATERAW, INFLATERAW), (GZIP, UNZIP), (DEFLATE, UNZIP), (BROTLI_ENCODE, BROTLI_DECODE), (ZSTD_COMPRESS, ZSTD_DECOMPRESS)] {
            let z = process(c, -1, &big).unwrap();
            assert!(z.len() < big.len(), "mode {c} did not compress");
            assert_eq!(process(d, -1, &z).unwrap(), big, "mode {c}->{d}");
        }
    }

    #[test]
    fn inflate_streams_byte_by_byte() {
        let z = process(GZIP, -1, b"hello hello hello world").unwrap();
        let mut s = ZStream::new(GUNZIP, -1, 0, vec![]);
        let mut out = Vec::new();
        for b in &z {
            out.extend(s.write(&[*b], Z_NO_FLUSH).unwrap());
        }
        out.extend(s.write(&[], Z_FINISH).unwrap());
        assert_eq!(out, b"hello hello hello world");
    }

    #[test]
    fn errors_match_node() {
        let msg = |mode, data: &[u8]| process(mode, -1, data).unwrap_err().message;
        assert_eq!(msg(INFLATE, b"hello"), "incorrect header check");
        assert_eq!(msg(GUNZIP, b"hello world"), "incorrect header check");
        let z = process(DEFLATE, -1, b"abc").unwrap();
        let e = process(INFLATE, -1, &z[..5]).unwrap_err();
        assert_eq!((e.message.as_str(), e.code.as_str(), e.errno), ("unexpected end of file", "Z_BUF_ERROR", -5));
        let mut bad = z.clone();
        *bad.last_mut().unwrap() ^= 1;
        assert_eq!(msg(INFLATE, &bad), "incorrect data check");
        let mut g = process(GZIP, -1, b"abc").unwrap();
        let n = g.len();
        g[n - 1] ^= 1;
        assert_eq!(msg(GUNZIP, &g), "incorrect length check");
        g[n - 1] ^= 1;
        g[n - 5] ^= 1;
        assert_eq!(msg(GUNZIP, &g), "incorrect data check");
        assert_eq!(msg(INFLATERAW, &[0xff, 0xff, 0xff]), "invalid block type");
        let e = process(BROTLI_DECODE, -1, b"garbage").unwrap_err();
        // Node (C brotli) reports PADDING_1 (-14) here; the Rust decoder notices PADDING_2.
        assert_eq!(e.message, "Decompression failed");
        assert!(e.code.starts_with("ERR__ERROR_FORMAT_") && e.errno < 0, "{e:?}");
    }

    #[test]
    fn gzip_members_and_trailing_data() {
        let mut two = process(GZIP, -1, b"abc").unwrap();
        two.extend(process(GZIP, -1, b"def").unwrap());
        assert_eq!(process(GUNZIP, -1, &two).unwrap(), b"abcdef");
        let mut garbage = process(GZIP, -1, b"abc").unwrap();
        garbage.extend_from_slice(b"garbage");
        assert_eq!(process(GUNZIP, -1, &garbage).unwrap_err().message, "incorrect header check");
        let mut padded = process(GZIP, -1, b"abc").unwrap();
        padded.extend_from_slice(&[0; 512]);
        assert_eq!(process(GUNZIP, -1, &padded).unwrap(), b"abc");
        let mut zlib_trailing = process(DEFLATE, -1, b"abc").unwrap();
        zlib_trailing.extend_from_slice(b"garbage");
        assert_eq!(process(INFLATE, -1, &zlib_trailing).unwrap(), b"abc");
    }

    fn hex(b: &[u8]) -> String {
        b.iter().map(|x| format!("{x:02x}")).collect()
    }
}
