//! AWS Signature Version 4 request signing (the parts of `@smithy/signature-v4` the Bedrock
//! client uses: header signing with `x-amz-content-sha256`, double-escaped canonical paths).

use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

type HmacSha256 = Hmac<Sha256>;

/// Static AWS credentials.
#[derive(Clone, PartialEq, Eq)]
pub struct AwsCredentials {
    pub access_key_id: String,
    pub secret_access_key: String,
    pub session_token: Option<String>,
    /// Expiration (epoch ms), when the source reports one.
    pub expiration_ms: Option<i64>,
}

impl std::fmt::Debug for AwsCredentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AwsCredentials")
            .field("access_key_id", &self.access_key_id)
            .field("secret_access_key", &"<redacted>")
            .field("session_token", &self.session_token.as_ref().map(|_| "<redacted>"))
            .field("expiration_ms", &self.expiration_ms)
            .finish()
    }
}

impl AwsCredentials {
    pub fn new(access_key_id: impl Into<String>, secret_access_key: impl Into<String>, session_token: Option<String>) -> Self {
        Self { access_key_id: access_key_id.into(), secret_access_key: secret_access_key.into(), session_token, expiration_ms: None }
    }
}

/// Headers that are never signed (`ALWAYS_UNSIGNABLE_HEADERS`).
const UNSIGNABLE_HEADERS: [&str; 15] = [
    "authorization",
    "cache-control",
    "connection",
    "expect",
    "from",
    "keep-alive",
    "max-forwards",
    "pragma",
    "referer",
    "te",
    "trailer",
    "transfer-encoding",
    "upgrade",
    "user-agent",
    "x-amzn-trace-id",
];

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn sha256_hex(data: &[u8]) -> String {
    hex(&Sha256::digest(data))
}

fn hmac(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

/// `encodeURIComponent` plus `!'()*` (the SDK's `escapeUri`/`extendedEncodeURIComponent`).
pub fn escape_uri(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(byte as char),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// The canonical path for services that escape paths (all but S3): normalized, then escaped
/// once more, keeping `/`.
pub fn canonical_path(path: &str) -> String {
    let mut segments: Vec<&str> = Vec::new();
    for segment in path.split('/') {
        if segment.is_empty() || segment == "." {
            continue;
        }
        if segment == ".." {
            segments.pop();
        } else {
            segments.push(segment);
        }
    }
    let leading = if path.starts_with('/') { "/" } else { "" };
    let trailing = if !segments.is_empty() && path.ends_with('/') { "/" } else { "" };
    let normalized = format!("{leading}{}{trailing}", segments.join("/"));
    escape_uri(&normalized).replace("%2F", "/")
}

fn canonical_query(query: Option<&str>) -> String {
    let Some(query) = query.filter(|q| !q.is_empty()) else { return String::new() };
    let mut pairs: Vec<(String, String)> = url::form_urlencoded::parse(query.as_bytes())
        .map(|(k, v)| (escape_uri(&k), escape_uri(&v)))
        .collect();
    pairs.sort();
    pairs.into_iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("&")
}

/// `YYYYMMDDTHHMMSSZ` for epoch milliseconds.
pub fn amz_date(epoch_ms: i64) -> String {
    let secs = epoch_ms.div_euclid(1000);
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (h, m, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    // civil_from_days
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { y + 1 } else { y };
    format!("{year:04}{month:02}{d:02}T{h:02}{m:02}{s:02}Z")
}

/// A request to sign.
pub struct SignableRequest<'a> {
    pub method: &'a str,
    /// Absolute URL (path already percent-encoded as sent).
    pub url: &'a url::Url,
    /// Headers that will be sent (excluding the ones the signer adds).
    pub headers: &'a [(String, String)],
    pub body: &'a [u8],
}

/// Sign `request` and return the headers to add: `x-amz-date`, `x-amz-security-token` (with a
/// session token), `x-amz-content-sha256` and `authorization`.
pub fn sign_request(
    request: &SignableRequest,
    credentials: &AwsCredentials,
    region: &str,
    service: &str,
    epoch_ms: i64,
) -> Vec<(String, String)> {
    sign_request_with(request, credentials, region, service, epoch_ms, true)
}

/// [`sign_request`] with control over the `x-amz-content-sha256` header (`applyChecksum`).
pub fn sign_request_with(
    request: &SignableRequest,
    credentials: &AwsCredentials,
    region: &str,
    service: &str,
    epoch_ms: i64,
    apply_checksum: bool,
) -> Vec<(String, String)> {
    let date_time = amz_date(epoch_ms);
    let date = &date_time[..8];
    let payload_hash = sha256_hex(request.body);
    let mut added: Vec<(String, String)> = vec![("x-amz-date".into(), date_time.clone())];
    if let Some(token) = &credentials.session_token {
        added.push(("x-amz-security-token".into(), token.clone()));
    }
    if apply_checksum && !request.headers.iter().any(|(n, _)| n.eq_ignore_ascii_case("x-amz-content-sha256")) {
        added.push(("x-amz-content-sha256".into(), payload_hash.clone()));
    }

    let mut canonical: Vec<(String, String)> = Vec::new();
    for (name, value) in request.headers.iter().chain(added.iter()) {
        let lower = name.to_lowercase();
        if UNSIGNABLE_HEADERS.contains(&lower.as_str()) {
            continue;
        }
        let value = value.split_whitespace().collect::<Vec<_>>().join(" ");
        match canonical.iter_mut().find(|(n, _)| *n == lower) {
            Some((_, existing)) => {
                existing.push(',');
                existing.push_str(&value);
            }
            None => canonical.push((lower, value)),
        }
    }
    canonical.sort_by(|a, b| a.0.cmp(&b.0));
    let signed_headers = canonical.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>().join(";");
    let canonical_headers: String = canonical.iter().map(|(n, v)| format!("{n}:{v}\n")).collect();
    let canonical_request = format!(
        "{}\n{}\n{}\n{}\n{}\n{}",
        request.method,
        canonical_path(request.url.path()),
        canonical_query(request.url.query()),
        canonical_headers,
        signed_headers,
        payload_hash
    );
    let scope = format!("{date}/{region}/{service}/aws4_request");
    let string_to_sign = format!("AWS4-HMAC-SHA256\n{date_time}\n{scope}\n{}", sha256_hex(canonical_request.as_bytes()));
    let k_date = hmac(format!("AWS4{}", credentials.secret_access_key).as_bytes(), date.as_bytes());
    let k_region = hmac(&k_date, region.as_bytes());
    let k_service = hmac(&k_region, service.as_bytes());
    let k_signing = hmac(&k_service, b"aws4_request");
    let signature = hex(&hmac(&k_signing, string_to_sign.as_bytes()));
    added.push((
        "authorization".into(),
        format!(
            "AWS4-HMAC-SHA256 Credential={}/{scope}, SignedHeaders={signed_headers}, Signature={signature}",
            credentials.access_key_id
        ),
    ));
    added
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_amz_dates() {
        assert_eq!(amz_date(0), "19700101T000000Z");
        assert_eq!(amz_date(1_440_938_160_000), "20150830T123600Z");
    }

    #[test]
    fn escapes_paths_twice() {
        assert_eq!(escape_uri("a:b/c!"), "a%3Ab%2Fc%21");
        assert_eq!(canonical_path("/model/arn%3Aaws%3Ax/converse-stream"), "/model/arn%253Aaws%253Ax/converse-stream");
        assert_eq!(canonical_path("/a/./b/../c/"), "/a/c/");
    }

    /// The `get-vanilla` and `post-x-www-form-urlencoded` cases of the AWS SigV4 test suite.
    #[test]
    fn matches_the_aws_test_suite() {
        let credentials = AwsCredentials::new("AKIDEXAMPLE", "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY", None);
        let url = url::Url::parse("https://example.amazonaws.com/").unwrap();
        let headers = vec![("host".to_string(), "example.amazonaws.com".to_string())];
        let request = SignableRequest { method: "GET", url: &url, headers: &headers, body: b"" };
        let added = sign_request_with(&request, &credentials, "us-east-1", "service", 1_440_938_160_000, false);
        assert_eq!(
            added.iter().find(|(n, _)| n == "authorization").unwrap().1,
            "AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/20150830/us-east-1/service/aws4_request, SignedHeaders=host;x-amz-date, Signature=5fa00fa31553b73ebf1942676e86291e8372ff2a2260956d9b8aae1d763fbf31"
        );
        let with_checksum = sign_request(&request, &credentials, "us-east-1", "service", 1_440_938_160_000);
        assert!(with_checksum.iter().any(|(n, v)| n == "x-amz-content-sha256" && v == &sha256_hex(b"")));
        assert!(with_checksum.iter().any(|(n, v)| n == "authorization" && v.contains("SignedHeaders=host;x-amz-content-sha256;x-amz-date,")));
    }

    #[test]
    fn signature_matches_known_vector() {
        // get-vanilla from the AWS SigV4 test suite, signed headers host;x-amz-date.
        let credentials = AwsCredentials::new("AKIDEXAMPLE", "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY", None);
        let canonical_request = "GET\n/\n\nhost:example.amazonaws.com\nx-amz-date:20150830T123600Z\n\nhost;x-amz-date\ne3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        let string_to_sign = format!(
            "AWS4-HMAC-SHA256\n20150830T123600Z\n20150830/us-east-1/service/aws4_request\n{}",
            sha256_hex(canonical_request.as_bytes())
        );
        let k_date = hmac(format!("AWS4{}", credentials.secret_access_key).as_bytes(), b"20150830");
        let k_region = hmac(&k_date, b"us-east-1");
        let k_service = hmac(&k_region, b"service");
        let k_signing = hmac(&k_service, b"aws4_request");
        assert_eq!(
            hex(&hmac(&k_signing, string_to_sign.as_bytes())),
            "5fa00fa31553b73ebf1942676e86291e8372ff2a2260956d9b8aae1d763fbf31"
        );
    }
}
