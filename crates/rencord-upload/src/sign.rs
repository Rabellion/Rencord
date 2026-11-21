//! AWS Signature Version 4 for the S3-compatible custom endpoint.
//!
//! The canonical request follows the published signing process. The secret
//! never appears in a header; only the access key and the signature do.

use crate::hex_encode;
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use std::time::{SystemTime, UNIX_EPOCH};

type HmacSha256 = Hmac<Sha256>;

pub struct SignInput<'a> {
    pub method: &'a str,
    pub url: &'a str,
    pub region: &'a str,
    pub service: &'a str,
    pub access_key: &'a str,
    pub secret_key: &'a str,
    pub payload: &'a [u8],
    pub content_type: &'a str,
}

pub fn sign_request(input: SignInput<'_>) -> (Vec<(String, String)>, String) {
    let amz_date = now_amz();
    sign_request_at(input, &amz_date)
}

pub fn sign_request_at(input: SignInput<'_>, amz_date: &str) -> (Vec<(String, String)>, String) {
    let (host, canonical_uri, canonical_query) = split_url(input.url);
    let date = &amz_date[..8.min(amz_date.len())];
    let payload_hash = hex_encode(&Sha256::digest(input.payload));
    let canonical_headers = format!(
        "content-type:{}\nhost:{host}\nx-amz-date:{amz_date}\n",
        input.content_type.trim()
    );
    let signed_headers = "content-type;host;x-amz-date";
    let canonical = format!(
        "{method}\n{canonical_uri}\n{canonical_query}\n{canonical_headers}\n{signed_headers}\n{payload_hash}",
        method = input.method,
    );
    let canonical_hash = hex_encode(&Sha256::digest(canonical.as_bytes()));
    let scope = format!("{date}/{}/{}/aws4_request", input.region, input.service);
    let string_to_sign = format!("AWS4-HMAC-SHA256\n{amz_date}\n{scope}\n{canonical_hash}");
    let signing_key = signing_key(input.secret_key, date, input.region, input.service);
    let signature = hex_encode(&hmac_sha256(&signing_key, string_to_sign.as_bytes()));
    let authorization = format!(
        "AWS4-HMAC-SHA256 Credential={}/{}, SignedHeaders={signed_headers}, Signature={signature}",
        input.access_key, scope
    );
    let headers = vec![
        ("Host".into(), host),
        ("Content-Type".into(), input.content_type.to_owned()),
        ("X-Amz-Date".into(), amz_date.to_owned()),
        ("Authorization".into(), authorization),
    ];
    (headers, signature)
}

fn signing_key(secret: &str, date: &str, region: &str, service: &str) -> Vec<u8> {
    let k_date = hmac_sha256(format!("AWS4{secret}").as_bytes(), date.as_bytes());
    let k_region = hmac_sha256(&k_date, region.as_bytes());
    let k_service = hmac_sha256(&k_region, service.as_bytes());
    hmac_sha256(&k_service, b"aws4_request")
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(key).expect("hmac-sha256 accepts any key length");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

fn split_url(url: &str) -> (String, String, String) {
    let rest = url
        .trim_start_matches("https://")
        .trim_start_matches("http://");
    let (host_and_path, query) = rest.split_once('?').unwrap_or((rest, ""));
    let (host, path) = host_and_path.split_once('/').unwrap_or((host_and_path, ""));
    let canonical_uri = if path.is_empty() {
        "/".to_owned()
    } else {
        format!("/{path}")
    };
    (host.to_owned(), canonical_uri, query.to_owned())
}

fn now_amz() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    let days = secs / 86_400;
    let tod = secs % 86_400;
    let (year, month, day) = crate::store::civil_from_days(days as i64);
    format!(
        "{year:04}{month:02}{day:02}T{:02}{:02}{:02}Z",
        tod / 3600,
        (tod % 3600) / 60,
        tod % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_the_published_iam_get_example() {
        // AWS general reference, "Examples of the Complete Version 4 Signing Process".
        // GET https://iam.amazonaws.com/?Action=ListUsers&Version=2010-05-08
        let (headers, signature) = sign_request_at(
            SignInput {
                method: "GET",
                url: "https://iam.amazonaws.com/?Action=ListUsers&Version=2010-05-08",
                region: "us-east-1",
                service: "iam",
                access_key: "AKIDEXAMPLE",
                secret_key: "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY",
                payload: b"",
                content_type: "application/x-www-form-urlencoded; charset=utf-8",
            },
            "20150830T123600Z",
        );
        assert_eq!(
            signature,
            "5d672d79c15b13162d9279b0855cfba6789a8edb4c82c400e06b5924a6f2b5d7"
        );
        let authorization = headers
            .iter()
            .find(|(name, _)| name == "Authorization")
            .map(|(_, value)| value.as_str())
            .unwrap();
        assert!(authorization.contains("SignedHeaders=content-type;host;x-amz-date"));
        assert!(!authorization.contains("wJalrXUtnFEMI"));
    }
}
