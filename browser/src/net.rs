use std::{
    fs::File,
    io::{Cursor, Read},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use blitz_traits::net::{Bytes, Method, NetHandler, NetProvider, Request, Url};
use tokio::{
    runtime::Handle,
    sync::{Semaphore, watch},
};

const PAGE_BYTES: usize = 2 * 1024 * 1024;
const RESOURCE_BYTES: usize = 2 * 1024 * 1024;
const TOTAL_RESOURCE_BYTES: usize = 12 * 1024 * 1024;
const MAX_RESOURCE_REQUESTS: usize = 48;
const MAX_CONCURRENT_RESOURCES: usize = 4;
const IMAGE_RGBA_BYTES: usize = 16 * 1024 * 1024;

pub type Wake = Arc<dyn Fn() + Send + Sync>;
pub type LoadResult = Result<Response, String>;

pub struct Response {
    pub url: Url,
    pub bytes: Vec<u8>,
    pub content_type: Option<String>,
}

impl Response {
    pub fn text(&self) -> String {
        let encoding = self.content_type.as_deref().and_then(|content_type| {
            let parameter = content_type.split(';').skip(1).find_map(|part| {
                let (name, value) = part.trim().split_once('=')?;
                name.trim().eq_ignore_ascii_case("charset").then_some(value)
            })?;
            encoding_rs::Encoding::for_label(parameter.trim().trim_matches(['"', '\'']).as_bytes())
        });
        encoding
            .unwrap_or(encoding_rs::UTF_8)
            .decode(&self.bytes)
            .0
            .into_owned()
    }
}

#[derive(Clone)]
pub struct Network {
    handle: Handle,
    client: reqwest::Client,
}

impl Network {
    pub fn new(handle: Handle) -> Result<Self, reqwest::Error> {
        let client = reqwest::Client::builder()
            .user_agent("MakerBrowser/0.1")
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(12))
            .pool_max_idle_per_host(0)
            .redirect(reqwest::redirect::Policy::limited(5))
            .build()?;
        Ok(Self { handle, client })
    }

    pub fn open(&self, url: Url, wake: Wake) -> Loading {
        let requests = PageRequests::new(self.clone(), &url, wake.clone());
        let (send, receive) = std::sync::mpsc::channel();
        let provider = requests.provider.clone();
        self.handle.spawn(async move {
            let result = provider.load(url, PAGE_BYTES, true).await;
            if send.send(result).is_ok() {
                wake();
            }
        });
        Loading { receive, requests }
    }
}

pub struct Loading {
    pub receive: std::sync::mpsc::Receiver<LoadResult>,
    pub requests: PageRequests,
}

pub struct PageRequests {
    provider: Arc<Resources>,
    // Dropping the sender cancels the main request and all requests for this page.
    _lifetime: watch::Sender<()>,
}

impl PageRequests {
    pub fn net_provider(&self) -> Arc<dyn NetProvider> {
        Arc::new(ResourceProvider(self.provider.clone()))
    }

    fn new(network: Network, url: &Url, wake: Wake) -> Self {
        let (lifetime, cancelled) = watch::channel(());
        let local_root = url
            .to_file_path()
            .ok()
            .and_then(|path| path.parent().map(Path::to_owned))
            .and_then(|path| path.canonicalize().ok());
        let provider = Arc::new(Resources {
            network,
            cancelled,
            local_root,
            wake,
            requests: AtomicUsize::new(0),
            bytes: AtomicUsize::new(0),
            image_bytes: AtomicUsize::new(0),
            in_flight: Semaphore::new(MAX_CONCURRENT_RESOURCES),
        });
        Self {
            provider,
            _lifetime: lifetime,
        }
    }
}

struct Resources {
    network: Network,
    cancelled: watch::Receiver<()>,
    local_root: Option<PathBuf>,
    wake: Wake,
    requests: AtomicUsize,
    bytes: AtomicUsize,
    image_bytes: AtomicUsize,
    in_flight: Semaphore,
}

impl Resources {
    async fn load(&self, url: Url, limit: usize, top_level: bool) -> LoadResult {
        let mut cancelled = self.cancelled.clone();
        tokio::select! {
            biased;
            _ = cancelled.changed() => Err("탭이 메모리에서 해제되었습니다.".into()),
            result = async {
                if top_level {
                    return self.fetch_url(url, limit, true).await;
                }
                let _permit = self.in_flight.acquire().await
                    .map_err(|_| "리소스 로드가 중단되었습니다.")?;
                let response = self.fetch_url(url, limit, false).await?;
                self.reserve_budget(&response)?;
                Ok(response)
            } => result,
        }
    }

    async fn fetch_url(&self, url: Url, limit: usize, top_level: bool) -> LoadResult {
        match url.scheme() {
            "http" | "https" => {
                let mut response = self
                    .network
                    .client
                    .get(url)
                    .send()
                    .await
                    .and_then(reqwest::Response::error_for_status)
                    .map_err(|error| error.to_string())?;
                if response
                    .content_length()
                    .is_some_and(|len| len > limit as u64)
                {
                    return Err("응답이 메모리 예산을 초과했습니다.".into());
                }
                let url = response.url().clone();
                let content_type = response
                    .headers()
                    .get(reqwest::header::CONTENT_TYPE)
                    .and_then(|value| value.to_str().ok())
                    .map(str::to_owned);
                let mut bytes = Vec::new();
                while let Some(chunk) = response.chunk().await.map_err(|error| error.to_string())? {
                    if bytes.len() + chunk.len() > limit {
                        return Err("응답이 메모리 예산을 초과했습니다.".into());
                    }
                    bytes.extend_from_slice(&chunk);
                }
                Ok(Response {
                    url,
                    bytes,
                    content_type,
                })
            }
            "file" => {
                let path = url
                    .to_file_path()
                    .map_err(|_| "올바른 로컬 경로가 아닙니다.")?;
                let root = self.local_root.clone();
                let bytes = tokio::task::spawn_blocking(move || {
                    let path = path.canonicalize()?;
                    if !top_level && !root.as_ref().is_some_and(|root| path.starts_with(root)) {
                        return Err(std::io::Error::other(
                            "HTML 파일의 폴더 안에 있는 리소스만 열 수 있습니다.",
                        ));
                    }
                    let mut bytes = Vec::new();
                    File::open(path)?
                        .take(limit as u64 + 1)
                        .read_to_end(&mut bytes)?;
                    if bytes.len() > limit {
                        return Err(std::io::Error::other("파일이 메모리 예산을 초과했습니다."));
                    }
                    Ok(bytes)
                })
                .await
                .map_err(|error| error.to_string())?
                .map_err(|error| error.to_string())?;
                Ok(Response {
                    url,
                    bytes,
                    content_type: None,
                })
            }
            "data" if !top_level => {
                if url.as_str().len() > limit {
                    return Err("데이터 URL이 메모리 예산을 초과했습니다.".into());
                }
                let data = data_url::DataUrl::process(url.as_str())
                    .map_err(|_| "올바른 데이터 URL이 아닙니다.")?;
                let (bytes, _) = data
                    .decode_to_vec()
                    .map_err(|_| "데이터 URL을 읽지 못했습니다.")?;
                Ok(Response {
                    url,
                    bytes,
                    content_type: None,
                })
            }
            _ => Err("이 주소 형식을 지원하지 않습니다.".into()),
        }
    }

    fn reserve_budget(&self, response: &Response) -> Result<(), String> {
        reserve(&self.bytes, response.bytes.len(), TOTAL_RESOURCE_BYTES)?;
        if image::guess_format(&response.bytes).is_ok() {
            let reader = image::ImageReader::new(Cursor::new(&response.bytes))
                .with_guessed_format()
                .map_err(|error| error.to_string())?;
            let (width, height) = reader
                .into_dimensions()
                .map_err(|error| error.to_string())?;
            let bytes = u64::from(width)
                .saturating_mul(u64::from(height))
                .saturating_mul(4);
            if bytes > IMAGE_RGBA_BYTES as u64 {
                return Err("이미지가 디코딩 메모리 예산을 초과했습니다.".into());
            }
            reserve(&self.image_bytes, bytes as usize, IMAGE_RGBA_BYTES)?;
        }
        Ok(())
    }
}

fn reserve(counter: &AtomicUsize, amount: usize, limit: usize) -> Result<(), String> {
    counter
        .try_update(Ordering::Relaxed, Ordering::Relaxed, |used| {
            used.checked_add(amount).filter(|total| *total <= limit)
        })
        .map(|_| ())
        .map_err(|_| "페이지 리소스가 메모리 예산을 초과했습니다.".into())
}

struct ResourceProvider(Arc<Resources>);

impl NetProvider for ResourceProvider {
    fn fetch(&self, _doc_id: usize, request: Request, handler: Box<dyn NetHandler>) {
        if request.method != Method::GET
            || reserve(&self.0.requests, 1, MAX_RESOURCE_REQUESTS).is_err()
        {
            handler.error(request.url.to_string(), "리소스 요청 제한".into());
            return;
        }
        let resources = self.0.clone();
        self.0.network.handle.spawn(async move {
            let original_url = request.url.to_string();
            let result = resources.load(request.url, RESOURCE_BYTES, false).await;
            if resources.cancelled.has_changed().is_err() {
                return;
            }
            match result {
                Ok(response) => {
                    handler.bytes(response.url.to_string(), Bytes::from(response.bytes))
                }
                Err(error) => handler.error(original_url, error),
            }
            (resources.wake)();
        });
    }
}

pub fn address(input: &str) -> Result<Url, String> {
    let input = input.trim().trim_matches('"');
    if input.eq_ignore_ascii_case("about:blank") {
        return Ok(blank());
    }
    if input.is_empty() {
        return Err("웹 주소나 HTML 파일 경로를 입력하세요.".into());
    }
    let path = Path::new(input);
    if path.is_absolute() || path.is_file() {
        let path = std::path::absolute(path).map_err(|error| error.to_string())?;
        return Url::from_file_path(path).map_err(|_| "올바른 로컬 경로가 아닙니다.".into());
    }
    let raw = if input.contains("://") || input.starts_with("file:") {
        input.to_owned()
    } else {
        format!("https://{input}")
    };
    let url = Url::parse(&raw).map_err(|error| error.to_string())?;
    if matches!(url.scheme(), "http" | "https" | "file")
        && url.username().is_empty()
        && url.password().is_none()
    {
        Ok(url)
    } else {
        Err("HTTP, HTTPS 또는 로컬 HTML 파일을 열 수 있습니다.".into())
    }
}

pub fn blank() -> Url {
    Url::parse("about:blank").expect("valid built-in URL")
}
