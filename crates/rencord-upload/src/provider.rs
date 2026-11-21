//! Request shapes for Litterbox, Catbox, 0x0.st, a presigned PUT, and S3.
//!
//! Building a request does not send it. [`crate::transfer::run_job`] is the
//! only path that opens a socket, and it refuses unless the caller passed
//! consent.

use crate::sign::sign_request;
use crate::UploadError;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::Path;

pub const USER_AGENT: &str = "Rencord (https://github.com/Rabellion/Rencord, 0.1.0)";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    Litterbox,
    Catbox,
    ZeroX,
    Custom,
}

impl ProviderKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Litterbox => "Litterbox",
            Self::Catbox => "Catbox",
            Self::ZeroX => "0x0.st",
            Self::Custom => "Custom",
        }
    }

    pub fn host(self) -> &'static str {
        match self {
            Self::Litterbox => "litterbox.catbox.moe",
            Self::Catbox => "catbox.moe",
            Self::ZeroX => "0x0.st",
            Self::Custom => "the custom endpoint",
        }
    }

    pub fn cycle(self) -> Self {
        match self {
            Self::Litterbox => Self::Catbox,
            Self::Catbox => Self::ZeroX,
            Self::ZeroX => Self::Custom,
            Self::Custom => Self::Litterbox,
        }
    }

    pub fn supports_resume(self) -> bool {
        matches!(self, Self::Custom)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LitterboxTime {
    H1,
    H12,
    H24,
    H72,
}

impl LitterboxTime {
    pub fn as_api(self) -> &'static str {
        match self {
            Self::H1 => "1h",
            Self::H12 => "12h",
            Self::H24 => "24h",
            Self::H72 => "72h",
        }
    }

    pub fn cycle(self) -> Self {
        match self {
            Self::H1 => Self::H12,
            Self::H12 => Self::H24,
            Self::H24 => Self::H72,
            Self::H72 => Self::H1,
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub enum CustomTarget {
    PresignedPut {
        url: String,
    },
    S3 {
        endpoint: String,
        bucket: String,
        region: String,
        access_key: String,
        secret_key: String,
        prefix: String,
        part_size: u64,
    },
}

impl fmt::Debug for CustomTarget {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PresignedPut { url } => formatter
                .debug_struct("PresignedPut")
                .field("url", url)
                .finish(),
            Self::S3 {
                endpoint,
                bucket,
                region,
                access_key,
                prefix,
                part_size,
                ..
            } => formatter
                .debug_struct("S3")
                .field("endpoint", endpoint)
                .field("bucket", bucket)
                .field("region", region)
                .field("access_key", access_key)
                .field("secret_key", &"[redacted]")
                .field("prefix", prefix)
                .field("part_size", part_size)
                .finish(),
        }
    }
}

impl CustomTarget {
    pub fn parse(spec: &str) -> Result<Self, UploadError> {
        let spec = spec.trim();
        if let Some(url) = spec.strip_prefix("put ") {
            let url = url.trim();
            if !url.starts_with("https://") && !url.starts_with("http://") {
                return Err(UploadError::message("presigned PUT needs an http(s) URL"));
            }
            return Ok(Self::PresignedPut {
                url: url.to_owned(),
            });
        }
        if let Some(rest) = spec.strip_prefix("s3 ") {
            let mut parts = rest.split_whitespace();
            let endpoint = parts
                .next()
                .ok_or_else(|| UploadError::message("s3 spec needs an endpoint"))?;
            let bucket = parts
                .next()
                .ok_or_else(|| UploadError::message("s3 spec needs a bucket"))?;
            let region = parts.next().unwrap_or("us-east-1");
            let access_key = parts
                .next()
                .ok_or_else(|| UploadError::message("s3 spec needs an access key"))?;
            let secret_key = parts
                .next()
                .ok_or_else(|| UploadError::message("s3 spec needs a secret key"))?;
            let prefix = parts.next().unwrap_or("").to_owned();
            return Ok(Self::S3 {
                endpoint: endpoint.trim_end_matches('/').to_owned(),
                bucket: bucket.to_owned(),
                region: region.to_owned(),
                access_key: access_key.to_owned(),
                secret_key: secret_key.to_owned(),
                prefix,
                part_size: 8 * 1024 * 1024,
            });
        }
        Err(UploadError::message(
            "custom endpoint must start with `put ` or `s3 `",
        ))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hosts {
    pub catbox: String,
    pub litterbox: String,
    pub zero_x: String,
}

impl Default for Hosts {
    fn default() -> Self {
        Self {
            catbox: "https://catbox.moe".into(),
            litterbox: "https://litterbox.catbox.moe".into(),
            zero_x: "https://0x0.st".into(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Method {
    Post,
    Put,
}

impl Method {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Post => "POST",
            Self::Put => "PUT",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FieldPlan {
    Text { name: String, value: String },
    File { name: String, filename: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BodyPlan {
    Multipart(Vec<FieldPlan>),
    RawFile { content_type: String },
    Empty,
    Xml(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HttpCall {
    pub method: Method,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: BodyPlan,
}

#[derive(Clone, Debug)]
pub struct UploadPlan {
    pub provider: ProviderKind,
    pub hosts: Hosts,
    pub litterbox: LitterboxTime,
    pub zero_x_hours: Option<u32>,
    pub custom: Option<CustomTarget>,
    pub file_name: String,
}

impl UploadPlan {
    pub fn calls(&self) -> Result<UploadRecipe, UploadError> {
        match self.provider {
            ProviderKind::Catbox => Ok(UploadRecipe::OneShot(self.catbox_call())),
            ProviderKind::Litterbox => Ok(UploadRecipe::OneShot(self.litterbox_call())),
            ProviderKind::ZeroX => Ok(UploadRecipe::OneShot(self.zero_x_call())),
            ProviderKind::Custom => self.custom_recipe(),
        }
    }

    fn catbox_call(&self) -> HttpCall {
        HttpCall {
            method: Method::Post,
            url: format!("{}/user/api.php", self.hosts.catbox.trim_end_matches('/')),
            headers: vec![("User-Agent".into(), USER_AGENT.into())],
            body: BodyPlan::Multipart(vec![
                FieldPlan::Text {
                    name: "reqtype".into(),
                    value: "fileupload".into(),
                },
                FieldPlan::File {
                    name: "fileToUpload".into(),
                    filename: self.file_name.clone(),
                },
            ]),
        }
    }

    fn litterbox_call(&self) -> HttpCall {
        HttpCall {
            method: Method::Post,
            url: format!(
                "{}/resources/internals/api.php",
                self.hosts.litterbox.trim_end_matches('/')
            ),
            headers: vec![("User-Agent".into(), USER_AGENT.into())],
            body: BodyPlan::Multipart(vec![
                FieldPlan::Text {
                    name: "reqtype".into(),
                    value: "fileupload".into(),
                },
                FieldPlan::Text {
                    name: "time".into(),
                    value: self.litterbox.as_api().into(),
                },
                FieldPlan::File {
                    name: "fileToUpload".into(),
                    filename: self.file_name.clone(),
                },
            ]),
        }
    }

    fn zero_x_call(&self) -> HttpCall {
        let mut fields = vec![FieldPlan::File {
            name: "file".into(),
            filename: self.file_name.clone(),
        }];
        if let Some(hours) = self.zero_x_hours {
            fields.push(FieldPlan::Text {
                name: "expires".into(),
                value: hours.to_string(),
            });
        }
        HttpCall {
            method: Method::Post,
            url: format!("{}/", self.hosts.zero_x.trim_end_matches('/')),
            headers: vec![("User-Agent".into(), USER_AGENT.into())],
            body: BodyPlan::Multipart(fields),
        }
    }

    fn custom_recipe(&self) -> Result<UploadRecipe, UploadError> {
        let Some(custom) = &self.custom else {
            return Err(UploadError::message(
                "set a custom endpoint before uploading",
            ));
        };
        match custom {
            CustomTarget::PresignedPut { url } => Ok(UploadRecipe::OneShot(HttpCall {
                method: Method::Put,
                url: url.clone(),
                headers: vec![
                    ("User-Agent".into(), USER_AGENT.into()),
                    ("Content-Type".into(), "application/octet-stream".into()),
                ],
                body: BodyPlan::RawFile {
                    content_type: "application/octet-stream".into(),
                },
            })),
            CustomTarget::S3 {
                endpoint,
                bucket,
                region,
                access_key,
                secret_key,
                prefix,
                part_size,
            } => {
                let key = object_key(prefix, &self.file_name);
                let object = format!(
                    "{}/{}/{}",
                    endpoint.trim_end_matches('/'),
                    encode_path(bucket),
                    encode_path(&key)
                );
                Ok(UploadRecipe::S3(S3Recipe {
                    object_url: object,
                    region: region.clone(),
                    access_key: access_key.clone(),
                    secret_key: secret_key.clone(),
                    part_size: (*part_size).max(1),
                    key,
                }))
            }
        }
    }
}

#[derive(Clone, Debug)]
pub enum UploadRecipe {
    OneShot(HttpCall),
    S3(S3Recipe),
}

#[derive(Clone)]
pub struct S3Recipe {
    pub object_url: String,
    pub region: String,
    pub access_key: String,
    pub secret_key: String,
    pub part_size: u64,
    pub key: String,
}

impl fmt::Debug for S3Recipe {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("S3Recipe")
            .field("object_url", &self.object_url)
            .field("region", &self.region)
            .field("access_key", &self.access_key)
            .field("secret_key", &"[redacted]")
            .field("part_size", &self.part_size)
            .field("key", &self.key)
            .finish()
    }
}

impl S3Recipe {
    pub fn initiate(&self) -> HttpCall {
        self.signed(
            Method::Post,
            "?uploads",
            b"",
            BodyPlan::Empty,
            "application/xml",
        )
    }

    pub fn part(&self, number: u32, upload_id: &str, payload: &[u8]) -> HttpCall {
        let query = format!("?partNumber={number}&uploadId={}", encode_query(upload_id));
        self.signed(
            Method::Put,
            &query,
            payload,
            BodyPlan::RawFile {
                content_type: "application/octet-stream".into(),
            },
            "application/octet-stream",
        )
    }

    pub fn complete(&self, upload_id: &str, parts: &[(u32, String)]) -> HttpCall {
        let query = format!("?uploadId={}", encode_query(upload_id));
        let xml = complete_xml(parts);
        self.signed(
            Method::Post,
            &query,
            xml.as_bytes(),
            BodyPlan::Xml(xml.clone()),
            "application/xml",
        )
    }

    fn signed(
        &self,
        method: Method,
        query: &str,
        payload: &[u8],
        body: BodyPlan,
        content_type: &str,
    ) -> HttpCall {
        let url = format!("{}{query}", self.object_url);
        let (headers, _) = sign_request(crate::sign::SignInput {
            method: method.as_str(),
            url: &url,
            region: &self.region,
            service: "s3",
            access_key: &self.access_key,
            secret_key: &self.secret_key,
            payload,
            content_type,
        });
        HttpCall {
            method,
            url,
            headers,
            body,
        }
    }
}

fn complete_xml(parts: &[(u32, String)]) -> String {
    let mut body = String::from("<CompleteMultipartUpload>");
    for (number, etag) in parts {
        body.push_str("<Part><PartNumber>");
        body.push_str(&number.to_string());
        body.push_str("</PartNumber><ETag>");
        body.push_str(&escape_xml(etag));
        body.push_str("</ETag></Part>");
    }
    body.push_str("</CompleteMultipartUpload>");
    body
}

fn escape_xml(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn object_key(prefix: &str, file_name: &str) -> String {
    let name = file_name.rsplit(['/', '\\']).next().unwrap_or("file");
    if prefix.is_empty() {
        name.to_owned()
    } else {
        format!("{}/{}", prefix.trim_matches('/'), name)
    }
}

fn encode_path(text: &str) -> String {
    let mut out = String::new();
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~' | b'/') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

fn encode_query(text: &str) -> String {
    encode_path(text).replace('/', "%2F")
}

#[derive(Clone)]
pub struct DiscordTarget {
    pub base: String,
    pub token: String,
    pub channel_id: u64,
}

impl fmt::Debug for DiscordTarget {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DiscordTarget")
            .field("base", &self.base)
            .field("channel_id", &self.channel_id)
            .field("token", &"[redacted]")
            .finish()
    }
}

pub fn discord_attachment_call(
    target: &DiscordTarget,
    content: &str,
    files: &[(String, String)],
) -> HttpCall {
    let mut fields = vec![FieldPlan::Text {
        name: "payload_json".into(),
        value: serde_json::json!({ "content": content }).to_string(),
    }];
    for (index, (filename, _)) in files.iter().enumerate() {
        fields.push(FieldPlan::File {
            name: format!("files[{index}]"),
            filename: filename.clone(),
        });
    }
    HttpCall {
        method: Method::Post,
        url: format!(
            "{}/channels/{}/messages",
            target.base.trim_end_matches('/'),
            target.channel_id
        ),
        headers: vec![
            ("Authorization".into(), target.token.clone()),
            ("User-Agent".into(), USER_AGENT.into()),
        ],
        body: BodyPlan::Multipart(fields),
    }
}

pub fn upload_call(plan: &UploadPlan) -> Result<UploadRecipe, UploadError> {
    plan.calls()
}

pub struct OwnedField {
    pub name: String,
    pub filename: Option<String>,
    pub value: Vec<u8>,
}

pub fn encode_multipart(boundary: &str, fields: &[OwnedField]) -> (String, Vec<u8>) {
    let mut body = Vec::new();
    for field in fields {
        body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        if let Some(filename) = &field.filename {
            body.extend_from_slice(
                format!(
                    "Content-Disposition: form-data; name=\"{}\"; filename=\"{}\"\r\nContent-Type: application/octet-stream\r\n\r\n",
                    field.name, filename
                )
                .as_bytes(),
            );
        } else {
            body.extend_from_slice(
                format!(
                    "Content-Disposition: form-data; name=\"{}\"\r\n\r\n",
                    field.name
                )
                .as_bytes(),
            );
        }
        body.extend_from_slice(&field.value);
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    (format!("multipart/form-data; boundary={boundary}"), body)
}

pub fn file_name_of(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("file")
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan(provider: ProviderKind) -> UploadPlan {
        UploadPlan {
            provider,
            hosts: Hosts::default(),
            litterbox: LitterboxTime::H1,
            zero_x_hours: Some(24),
            custom: None,
            file_name: "clip.mp4".into(),
        }
    }

    fn multipart(call: &HttpCall) -> &[FieldPlan] {
        match &call.body {
            BodyPlan::Multipart(fields) => fields,
            other => panic!("expected multipart, got {other:?}"),
        }
    }

    #[test]
    fn catbox_litterbox_and_zero_x_fields() {
        let catbox = plan(ProviderKind::Catbox).calls().unwrap();
        let UploadRecipe::OneShot(call) = catbox else {
            panic!("catbox is one shot");
        };
        assert_eq!(call.method, Method::Post);
        assert!(call.url.ends_with("/user/api.php"));
        let fields = multipart(&call);
        assert!(fields.iter().any(|field| matches!(field, FieldPlan::Text { name, value } if name == "reqtype" && value == "fileupload")));
        assert!(fields
            .iter()
            .any(|field| matches!(field, FieldPlan::File { name, .. } if name == "fileToUpload")));

        let litter = plan(ProviderKind::Litterbox).calls().unwrap();
        let UploadRecipe::OneShot(call) = litter else {
            panic!("litterbox is one shot");
        };
        assert!(call.url.contains("litterbox.catbox.moe"));
        assert!(multipart(&call).iter().any(|field| matches!(field, FieldPlan::Text { name, value } if name == "time" && value == "1h")));

        let zero = plan(ProviderKind::ZeroX).calls().unwrap();
        let UploadRecipe::OneShot(call) = zero else {
            panic!("0x0 is one shot");
        };
        assert!(call.url.starts_with("https://0x0.st"));
        assert!(call
            .headers
            .iter()
            .any(|(name, value)| name == "User-Agent" && value.contains("Rencord")));
        assert!(multipart(&call).iter().any(|field| matches!(field, FieldPlan::Text { name, value } if name == "expires" && value == "24")));
    }

    #[test]
    fn presigned_put_and_s3_urls() {
        let mut plan = plan(ProviderKind::Custom);
        plan.custom = Some(CustomTarget::PresignedPut {
            url: "https://files.example/put-me".into(),
        });
        let UploadRecipe::OneShot(call) = plan.calls().unwrap() else {
            panic!("put");
        };
        assert_eq!(call.method, Method::Put);
        assert_eq!(call.url, "https://files.example/put-me");

        plan.custom = Some(
            CustomTarget::parse("s3 https://s3.example bucket us-east-1 AKIA s3cr3t-value prefix")
                .unwrap(),
        );
        if let CustomTarget::S3 { part_size, .. } = plan.custom.as_ref().unwrap() {
            assert_eq!(*part_size, 8 * 1024 * 1024);
        }
        let UploadRecipe::S3(recipe) = plan.calls().unwrap() else {
            panic!("s3");
        };
        let initiate = recipe.initiate();
        assert!(initiate.url.contains("/bucket/prefix/clip.mp4?uploads"));
        assert!(initiate
            .headers
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case("authorization")));
        let part = recipe.part(2, "upload-1", b"part-bytes");
        assert!(part.url.contains("partNumber=2"));
        assert!(part.url.contains("uploadId=upload-1"));
        let rendered = format!("{:?}", plan.custom.as_ref().unwrap());
        assert!(rendered.contains("[redacted]"));
        assert!(!rendered.contains("s3cr3t-value"));
    }

    #[test]
    fn discord_attachment_request_names_the_files() {
        let call = discord_attachment_call(
            &DiscordTarget {
                base: "http://127.0.0.1:9".into(),
                token: "secret-token".into(),
                channel_id: 42,
            },
            "rencord-split v1",
            &[("a.zip.001".into(), "ignored".into())],
        );
        assert!(call.url.ends_with("/channels/42/messages"));
        let fields = multipart(&call);
        assert!(fields.iter().any(|field| matches!(field, FieldPlan::Text { name, value } if name == "payload_json" && value.contains("rencord-split"))));
        assert!(fields.iter().any(|field| matches!(field, FieldPlan::File { name, filename } if name == "files[0]" && filename == "a.zip.001")));
        let rendered = format!(
            "{:?}",
            DiscordTarget {
                base: "http://x".into(),
                token: "secret-token".into(),
                channel_id: 1,
            }
        );
        assert!(!rendered.contains("secret-token"));
    }
}
