use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
mod context;

const BODY_LIMIT: usize = 8 * 1024 * 1024;
const HEADER_LIMIT: usize = 16 * 1024;
const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

#[derive(Debug, Serialize, PartialEq)]
pub struct DockerCategory {
    category: &'static str,
    count: Option<u64>,
    size: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct DockerStorage {
    endpoint: String,
    categories: Vec<DockerCategory>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Usage {
    total_count: u64,
    total_size: i64,
}

#[derive(Deserialize)]
struct SizedItem {
    #[serde(rename = "Size")]
    size: Option<i64>,
}

#[derive(Deserialize)]
struct VolumeItem {
    #[serde(rename = "UsageData")]
    usage: Option<SizedItem>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct CacheItem {
    size: Option<i64>,
    shared: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct DiskUsage {
    image_usage: Option<Usage>,
    container_usage: Option<Usage>,
    volume_usage: Option<Usage>,
    build_cache_usage: Option<Usage>,
    layers_size: Option<i64>,
    images: Option<Vec<serde::de::IgnoredAny>>,
    containers: Option<Vec<ContainerItem>>,
    volumes: Option<Vec<VolumeItem>>,
    build_cache: Option<Vec<CacheItem>>,
}

#[derive(Deserialize)]
struct ContainerItem {
    #[serde(rename = "SizeRw")]
    size: Option<i64>,
}

fn measured(value: i64) -> Result<Option<u64>, String> {
    match value {
        -1 => Ok(None),
        0.. => Ok(Some(value as u64)),
        _ => Err("Docker returned an invalid size.".into()),
    }
}

fn sum_sizes(values: impl Iterator<Item = Option<i64>>) -> Result<Option<u64>, String> {
    let mut total = 0_u64;
    let mut known = true;
    for value in values {
        match value.map(measured).transpose()?.flatten() {
            Some(size) => total = total.checked_add(size).ok_or("Docker size overflow.")?,
            None => known = false,
        }
    }
    Ok(known.then_some(total))
}

fn category(
    name: &'static str,
    current: Option<Usage>,
    legacy_count: Option<usize>,
    legacy_size: Option<u64>,
) -> Result<DockerCategory, String> {
    let (count, size) = match current {
        Some(usage) => (Some(usage.total_count), measured(usage.total_size)?),
        None => (legacy_count.map(|n| n as u64), legacy_size),
    };
    if count.is_some_and(|value| value > MAX_SAFE_INTEGER)
        || size.is_some_and(|value| value > MAX_SAFE_INTEGER)
    {
        return Err("Docker measurement exceeds the interface's exact numeric range.".into());
    }
    Ok(DockerCategory {
        category: name,
        count,
        size,
    })
}

fn parse_usage(body: &[u8]) -> Result<Vec<DockerCategory>, String> {
    let usage: DiskUsage = serde_json::from_slice(body)
        .map_err(|_| "Docker returned invalid disk usage data.".to_string())?;
    let containers = usage.containers.as_ref();
    let volumes = usage.volumes.as_ref();
    let cache = usage.build_cache.as_ref();
    let rows = vec![
        category(
            "images",
            usage.image_usage,
            usage.images.as_ref().map(Vec::len),
            usage.layers_size.map(measured).transpose()?.flatten(),
        )?,
        category(
            "containers",
            usage.container_usage,
            containers.map(Vec::len),
            containers
                .map(|items| sum_sizes(items.iter().map(|item| item.size)))
                .transpose()?
                .flatten(),
        )?,
        category(
            "volumes",
            usage.volume_usage,
            volumes.map(Vec::len),
            volumes
                .map(|items| {
                    sum_sizes(
                        items
                            .iter()
                            .map(|item| item.usage.as_ref().and_then(|u| u.size)),
                    )
                })
                .transpose()?
                .flatten(),
        )?,
        category(
            "build_cache",
            usage.build_cache_usage,
            cache.map(Vec::len),
            cache
                .map(|items| {
                    sum_sizes(
                        items
                            .iter()
                            .filter(|item| !item.shared)
                            .map(|item| item.size),
                    )
                })
                .transpose()?
                .flatten(),
        )?,
    ];
    if rows
        .iter()
        .all(|row| row.count.is_none() && row.size.is_none())
    {
        return Err("Docker returned no supported disk usage data.".into());
    }
    Ok(rows)
}

fn endpoint(host: Option<&str>) -> Result<String, String> {
    let host = host
        .filter(|s| !s.is_empty())
        .unwrap_or("npipe:////./pipe/docker_engine");
    let name = host
        .strip_prefix("npipe:////./pipe/")
        .or_else(|| host.strip_prefix("npipe://./pipe/"))
        .ok_or("Docker analysis requires a local named pipe.")?;
    if name.is_empty()
        || name.len() > 200
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
    {
        return Err("Docker named pipe is invalid.".into());
    }
    Ok(format!(r"\\.\pipe\{name}"))
}

fn api_version(body: &[u8]) -> Result<String, String> {
    #[derive(Deserialize)]
    struct Version {
        #[serde(rename = "ApiVersion")]
        api: String,
        #[serde(rename = "MinAPIVersion")]
        minimum: Option<String>,
    }
    fn minor(value: &str) -> Result<u32, String> {
        value
            .strip_prefix("1.")
            .and_then(|n| n.parse().ok())
            .ok_or_else(|| "Docker API version is unsupported.".into())
    }
    let version: Version =
        serde_json::from_slice(body).map_err(|_| "Docker returned an invalid API version.")?;
    let selected = minor(&version.api)?.min(56);
    if selected < 31
        || version
            .minimum
            .as_deref()
            .map(minor)
            .transpose()?
            .is_some_and(|minimum| minimum > selected)
    {
        return Err("Docker API version is unsupported.".into());
    }
    Ok(format!("1.{selected}"))
}

fn http_body(response: &[u8]) -> Result<Vec<u8>, String> {
    let mut headers = [httparse::EMPTY_HEADER; 64];
    let mut parsed = httparse::Response::new(&mut headers);
    let offset = match parsed
        .parse(response)
        .map_err(|_| "Docker returned invalid HTTP headers.")?
    {
        httparse::Status::Complete(offset) if offset <= HEADER_LIMIT => offset,
        _ => return Err("Docker returned incomplete or oversized HTTP headers.".into()),
    };
    if parsed.code != Some(200) {
        return Err(format!(
            "Docker API request failed (HTTP {}).",
            parsed.code.unwrap_or(0)
        ));
    }
    let mut length = None;
    let mut chunked = false;
    for header in parsed.headers.iter() {
        if header.name.eq_ignore_ascii_case("content-length") {
            if length.is_some() {
                return Err("Docker returned duplicate content lengths.".into());
            }
            length = Some(
                std::str::from_utf8(header.value)
                    .ok()
                    .and_then(|s| s.parse::<usize>().ok())
                    .ok_or("Docker returned an invalid content length.")?,
            );
        } else if header.name.eq_ignore_ascii_case("transfer-encoding") {
            if chunked || !header.value.eq_ignore_ascii_case(b"chunked") {
                return Err("Docker returned an unsupported transfer encoding.".into());
            }
            chunked = true;
        }
    }
    let body = &response[offset..];
    if chunked {
        if length.is_some() {
            return Err("Docker returned ambiguous HTTP framing.".into());
        }
        let mut remaining = body;
        let mut decoded = Vec::new();
        loop {
            let (line, size) = match httparse::parse_chunk_size(remaining)
                .map_err(|_| "Docker returned invalid HTTP chunks.")?
            {
                httparse::Status::Complete(chunk) => chunk,
                _ => return Err("Docker returned incomplete HTTP chunks.".into()),
            };
            let size = usize::try_from(size).map_err(|_| "Docker response is too large.")?;
            remaining = &remaining[line..];
            if size == 0 {
                // Docker sends no trailers; reject them rather than interpreting unchecked data.
                if remaining != b"\r\n" {
                    return Err("Docker returned unsupported HTTP trailers.".into());
                }
                return Ok(decoded);
            }
            if size > BODY_LIMIT - decoded.len() {
                return Err("Docker response is too large.".into());
            }
            if remaining.len() < size + 2 || &remaining[size..size + 2] != b"\r\n" {
                return Err("Docker returned incomplete HTTP chunks.".into());
            }
            decoded.extend_from_slice(&remaining[..size]);
            remaining = &remaining[size + 2..];
        }
    }
    if body.len() > BODY_LIMIT || length.is_some_and(|size| size != body.len()) {
        return Err("Docker returned incomplete or oversized data.".into());
    }
    Ok(body.to_vec())
}

#[cfg(windows)]
async fn get(pipe: &str, path: &str) -> Result<Vec<u8>, String> {
    use tokio::net::windows::named_pipe::ClientOptions;
    let mut client = loop {
        match ClientOptions::new().open(pipe) {
            Ok(client) => break client,
            Err(error) if error.raw_os_error() == Some(231) => {
                tokio::time::sleep(Duration::from_millis(50)).await
            }
            Err(error) => {
                return Err(match error.raw_os_error() {
                    Some(2) => "Docker is not running on the selected local pipe.".into(),
                    Some(5) => "Docker access was denied.".into(),
                    _ => format!("Docker connection failed: {error}"),
                });
            }
        }
    };
    client.write_all(format!("GET {path} HTTP/1.1\r\nHost: docker\r\nConnection: close\r\nAccept: application/json\r\n\r\n").as_bytes())
        .await.map_err(|_| "Docker request could not be sent.")?;
    let mut response = Vec::new();
    let mut buffer = [0; 8192];
    let mut headers_received = false;
    loop {
        let count = match client.read(&mut buffer).await {
            Ok(count) => count,
            Err(error) if error.kind() == std::io::ErrorKind::BrokenPipe => 0,
            Err(_) => return Err("Docker response could not be read.".into()),
        };
        if count == 0 {
            break;
        }
        if response.len() + count > BODY_LIMIT + 1024 * 1024 {
            return Err("Docker response is too large.".into());
        }
        response.extend_from_slice(&buffer[..count]);
        if !headers_received {
            headers_received = response.windows(4).any(|bytes| bytes == b"\r\n\r\n");
            if !headers_received && response.len() > HEADER_LIMIT {
                return Err("Docker HTTP headers are too large.".into());
            }
        }
    }
    http_body(&response)
}

#[tauri::command]
pub async fn query_docker_storage(app: tauri::AppHandle) -> Result<DockerStorage, String> {
    #[cfg(windows)]
    {
        use tauri::Manager;
        let home = app.path().home_dir().ok();
        tokio::time::timeout(Duration::from_secs(15), async {
            let pipe = tauri::async_runtime::spawn_blocking(move || {
                context::resolve(home, |name| std::env::var_os(name))
            })
            .await
            .map_err(|_| "Docker configuration task failed.".to_string())??;
            let version = api_version(&get(&pipe, "/version").await?)?;
            let categories = parse_usage(&get(&pipe, &format!("/v{version}/system/df")).await?)?;
            Ok(DockerStorage {
                endpoint: pipe,
                categories,
            })
        })
        .await
        .map_err(|_| "Docker analysis timed out.".to_string())?
    }
    #[cfg(not(windows))]
    Err("Docker named pipe analysis is available on Windows.".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn named_pipe_get_is_read_only_and_silent_server_times_out() {
        use tokio::net::windows::named_pipe::ServerOptions;
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let pipe = format!(r"\\.\pipe\voldiq-docker-test-{}", std::process::id());
                let mut server = ServerOptions::new()
                    .first_pipe_instance(true)
                    .create(&pipe)
                    .unwrap();
                let fixture = tempfile::tempdir().unwrap();
                let metadata = fixture.path().join("contexts/meta/fe9c6bd7a66301f49ca9b6a70b217107cd1284598bfc254700c989b916da791e");
                std::fs::create_dir_all(&metadata).unwrap();
                std::fs::write(fixture.path().join("config.json"), br#"{"currentContext":"desktop-linux"}"#).unwrap();
                std::fs::write(metadata.join("meta.json"), serde_json::to_vec(&serde_json::json!({
                    "Name": "desktop-linux", "Endpoints": {"docker": {"Host": format!("npipe://./pipe/voldiq-docker-test-{}", std::process::id())}}
                })).unwrap()).unwrap();
                let selected_pipe = context::resolve(None, |name| {
                    (name == "DOCKER_CONFIG").then(|| fixture.path().as_os_str().to_owned())
                }).unwrap();
                assert_eq!(selected_pipe, pipe);
                let task = tokio::spawn(async move {
                    server.connect().await.unwrap();
                    let mut request = Vec::new();
                    let mut buffer = [0; 1024];
                    while !request.windows(4).any(|b| b == b"\r\n\r\n") {
                        let count = server.read(&mut buffer).await.unwrap();
                        assert!(count > 0 && request.len() + count < 4096);
                        request.extend_from_slice(&buffer[..count]);
                    }
                    assert!(request.starts_with(b"GET /v1.56/system/df HTTP/1.1\r\n"));
                    server
                        .write_all(
                            b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}",
                        )
                        .await
                        .unwrap();
                    tokio::time::sleep(Duration::from_millis(20)).await;
                });
                assert_eq!(
                    tokio::time::timeout(Duration::from_secs(2), get(&selected_pipe, "/v1.56/system/df"))
                        .await
                        .unwrap()
                        .unwrap(),
                    b"{}"
                );
                task.await.unwrap();
                let silent_pipe = format!("{pipe}-silent");
                let server = ServerOptions::new()
                    .first_pipe_instance(true)
                    .create(&silent_pipe)
                    .unwrap();
                assert!(
                    tokio::time::timeout(Duration::from_millis(50), get(&silent_pipe, "/version"))
                        .await
                        .is_err()
                );
                drop(server);
            });
    }

    #[test]
    fn current_and_legacy_usage_preserve_unknown_and_shared_sizes() {
        let current = parse_usage(br#"{"ImageUsage":{"TotalCount":2,"TotalSize":100},"ContainerUsage":{"TotalCount":1,"TotalSize":7},"VolumeUsage":{"TotalCount":1,"TotalSize":-1},"BuildCacheUsage":{"TotalCount":3,"TotalSize":50}}"#).unwrap();
        assert_eq!(
            current.iter().map(|r| r.size).collect::<Vec<_>>(),
            [Some(100), Some(7), None, Some(50)]
        );
        let legacy = parse_usage(br#"{"LayersSize":100,"Images":[{"Size":100},{"Size":100}],"Containers":[{"SizeRw":7,"SizeRootFs":1000}],"Volumes":[{"UsageData":{"Size":4}},{"UsageData":{"Size":-1}}],"BuildCache":[{"Size":9,"Shared":true},{"Size":5,"Shared":false}]}"#).unwrap();
        assert_eq!(
            legacy.iter().map(|r| r.size).collect::<Vec<_>>(),
            [Some(100), Some(7), None, Some(5)]
        );
        assert_eq!(legacy[0].count, Some(2));
        assert_eq!(legacy[3].count, Some(2));
        assert!(parse_usage(b"{}").is_err());
        assert!(parse_usage(br#"{"ImageUsage":{"TotalCount":1,"TotalSize":-2}}"#).is_err());
        assert!(parse_usage(br#"{"ImageUsage":{"TotalSize":1}}"#).is_err());
        assert!(
            parse_usage(br#"{"ImageUsage":{"TotalCount":1,"TotalSize":9007199254740992}}"#)
                .is_err()
        );
        assert!(sum_sizes([Some(i64::MAX), Some(i64::MAX), Some(3)].into_iter()).is_err());
    }

    #[test]
    fn endpoint_and_version_reject_remote_or_untrusted_values() {
        assert_eq!(endpoint(None).unwrap(), r"\\.\pipe\docker_engine");
        assert_eq!(
            endpoint(Some("npipe://./pipe/dockerDesktopLinuxEngine")).unwrap(),
            r"\\.\pipe\dockerDesktopLinuxEngine"
        );
        for host in [
            "tcp://localhost:2375",
            "npipe:////server/pipe/docker",
            "npipe:////./pipe/a/b",
            "npipe:////./pipe/a\r\n",
        ] {
            assert!(endpoint(Some(host)).is_err());
        }
        assert_eq!(
            api_version(br#"{"ApiVersion":"1.56","MinAPIVersion":"1.44"}"#).unwrap(),
            "1.56"
        );
        assert_eq!(api_version(br#"{"ApiVersion":"1.51"}"#).unwrap(), "1.51");
        assert!(api_version(br#"{"ApiVersion":"1.60","MinAPIVersion":"1.57"}"#).is_err());
        assert!(api_version(br#"{"ApiVersion":"1.56\r\nInjected"}"#).is_err());
    }

    #[test]
    fn http_framing_rejects_partial_ambiguous_and_oversized_responses() {
        assert_eq!(
            http_body(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}").unwrap(),
            b"{}"
        );
        assert_eq!(http_body(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n1\r\n{\r\n1\r\n}\r\n0\r\n\r\n").unwrap(), b"{}");
        for response in [
            b"HTTP/1.1 500 Bad\r\n\r\n{}".as_slice(),
            b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\n\r\n{}",
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nContent-Length: 2\r\n\r\n{}",
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n2\r\n{",
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nffffffff\r\n",
        ] {
            assert!(http_body(response).is_err());
        }
    }
}
