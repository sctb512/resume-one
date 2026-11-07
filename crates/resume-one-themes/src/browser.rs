//! Chromium is a disposable, non-interactive HTML painter, separate from QuickJS.
use std::{
    fs, io,
    net::{Ipv4Addr, SocketAddr, TcpStream},
    path::PathBuf,
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use tempfile::TempDir;
use tungstenite::{Message, WebSocket, client::client_with_config, protocol::WebSocketConfig};

use crate::{OriginalTheme, ThemeError, original::RenderedHtml};

const VIEWPORT_WIDTH: u32 = 794;
const VIEWPORT_HEIGHT: u32 = 1123;
const MAX_PREVIEW_HEIGHT: u32 = 16_000;
const TIMEOUT: Duration = Duration::from_secs(12);

pub struct OriginalPreview {
    pub strips: Vec<(u32, Vec<u8>)>,
    pub width: u32,
}

pub fn preview_original(
    theme: &OriginalTheme,
    resume: &Value,
) -> Result<OriginalPreview, ThemeError> {
    let rendered = theme.render_html(resume)?;
    let mut browser = Browser::open(&rendered.html, &theme.stylesheets)?;
    let bounds = browser.call("Page.getLayoutMetrics", json!({}))?;
    let height = bounds["cssContentSize"]["height"]
        .as_f64()
        .filter(|height| height.is_finite() && *height <= f64::from(MAX_PREVIEW_HEIGHT))
        .ok_or_else(|| {
            ThemeError::new(
                "ORIGINAL_PREVIEW_SIZE",
                "原版主题预览超过 16000 像素，请使用 PDF 查看完整内容",
            )
        })?
        .ceil() as u32;
    let height = height.max(VIEWPORT_HEIGHT);
    let mut strips = Vec::new();
    for y in (0..height).step_by(VIEWPORT_HEIGHT as usize) {
        let strip_height = (height - y).min(VIEWPORT_HEIGHT);
        let response = browser.call("Page.captureScreenshot", json!({
            "format": "png", "captureBeyondViewport": true,
            "clip": {"x": 0, "y": y, "width": VIEWPORT_WIDTH, "height": strip_height, "scale": 1}
        }))?;
        strips.push((strip_height, decode(&response)?));
    }
    Ok(OriginalPreview {
        strips,
        width: VIEWPORT_WIDTH,
    })
}

pub fn print_original(theme: &OriginalTheme, resume: &Value) -> Result<Vec<u8>, ThemeError> {
    let rendered = theme.render_html(resume)?;
    let mut browser = Browser::open(&rendered.html, &theme.stylesheets)?;
    let response = browser.call("Page.printToPDF", print_options(&rendered))?;
    decode(&response)
}

fn dimension(value: &Value) -> Option<f64> {
    let inches = if let Some(px) = value.as_f64() {
        px / 96.
    } else {
        let text = value.as_str()?.trim();
        let (number, factor) = [
            ("mm", 1. / 25.4),
            ("cm", 1. / 2.54),
            ("in", 1.),
            ("px", 1. / 96.),
        ]
        .into_iter()
        .find_map(|(unit, factor)| text.strip_suffix(unit).map(|number| (number, factor)))?;
        number.trim().parse::<f64>().ok()? * factor
    };
    (0.0..=40.0).contains(&inches).then_some(inches)
}

fn print_options(rendered: &RenderedHtml) -> Value {
    let options = &rendered.print;
    let (width, height) = match options["format"]
        .as_str()
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("letter") => (8.5, 11.),
        Some("legal") => (8.5, 14.),
        _ => (210. / 25.4, 297. / 25.4),
    };
    let margin = |side: &str| dimension(&options["margin"][side]).unwrap_or(0.);
    json!({
        "printBackground": options["printBackground"].as_bool().unwrap_or(true),
        "preferCSSPageSize": options["preferCSSPageSize"].as_bool().unwrap_or(true),
        "landscape": options["landscape"].as_bool().unwrap_or(false),
        "scale": options["scale"].as_f64().filter(|scale| (0.1..=2.0).contains(scale)).unwrap_or(1.),
        "paperWidth": dimension(&options["width"]).unwrap_or(width),
        "paperHeight": dimension(&options["height"]).unwrap_or(height),
        "marginTop": margin("top"), "marginRight": margin("right"),
        "marginBottom": margin("bottom"), "marginLeft": margin("left")
    })
}

fn decode(response: &Value) -> Result<Vec<u8>, ThemeError> {
    response["data"]
        .as_str()
        .and_then(|data| STANDARD.decode(data).ok())
        .ok_or_else(|| browser_error("浏览器未返回有效的渲染数据"))
}

fn browser_error(message: &'static str) -> ThemeError {
    ThemeError::new("ORIGINAL_BROWSER", message)
}

fn executable() -> Option<PathBuf> {
    if let Some(explicit) = std::env::var_os("RESUME_ONE_CHROMIUM") {
        let path = PathBuf::from(explicit);
        return (path.is_absolute() && path.is_file()).then_some(path);
    }
    #[cfg(target_os = "macos")]
    let candidates = [
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
        "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
        "/Applications/Chromium.app/Contents/MacOS/Chromium",
    ]
    .map(PathBuf::from)
    .into_iter()
    .collect::<Vec<_>>();
    #[cfg(target_os = "windows")]
    let candidates = ["ProgramFiles(x86)", "ProgramFiles", "LOCALAPPDATA"]
        .into_iter()
        .filter_map(std::env::var_os)
        .flat_map(|base| {
            [
                std::path::Path::new(&base).join(r"Microsoft\Edge\Application\msedge.exe"),
                std::path::Path::new(&base).join(r"Google\Chrome\Application\chrome.exe"),
            ]
        })
        .collect::<Vec<_>>();
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let candidates = [
        "/usr/bin/chromium",
        "/usr/bin/google-chrome",
        "/usr/bin/chromium-browser",
    ]
    .map(PathBuf::from)
    .into_iter()
    .collect::<Vec<_>>();
    candidates.into_iter().find(|path| path.is_file())
}

struct PainterProcess {
    child: Child,
    profile: TempDir,
}

impl Drop for PainterProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl PainterProcess {
    fn launch() -> Result<Self, ThemeError> {
        let binary = executable().ok_or_else(|| {
            ThemeError::new(
                "ORIGINAL_BROWSER_MISSING",
                "原版主题需要本机 Chromium、Chrome 或 Edge",
            )
        })?;
        let profile =
            tempfile::tempdir().map_err(|_| browser_error("无法创建隔离的浏览器配置目录"))?;
        let mut profile_arg = std::ffi::OsString::from("--user-data-dir=");
        profile_arg.push(profile.path());
        let mut command = Command::new(binary);
        command
            .args([
                "--headless",
                "--remote-debugging-port=0",
                "--remote-debugging-address=127.0.0.1",
                "--no-first-run",
                "--no-default-browser-check",
                "--disable-extensions",
                "--disable-background-networking",
                "--disable-sync",
                "--disable-component-update",
                "--disable-default-apps",
                "--disable-breakpad",
                "--proxy-server=http://127.0.0.1:9",
                "--proxy-bypass-list=<-loopback>",
                "--host-resolver-rules=MAP * ~NOTFOUND",
            ])
            .arg(profile_arg)
            .arg("about:blank")
            .env_remove("RESUME_ONE_THEME_GITHUB_TOKEN")
            .env_remove("GITHUB_TOKEN")
            .env_remove("GH_TOKEN")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000); // CREATE_NO_WINDOW
        }
        let child = command
            .spawn()
            .map_err(|_| browser_error("无法启动隔离的浏览器进程"))?;
        Ok(Self { child, profile })
    }

    fn port(&mut self, deadline: Instant) -> Result<u16, ThemeError> {
        let path = self.profile.path().join("DevToolsActivePort");
        loop {
            if let Ok(contents) = fs::read_to_string(&path)
                && let Some(port) = contents.lines().next().and_then(|line| line.parse().ok())
            {
                return Ok(port);
            }
            if Instant::now() >= deadline || !matches!(self.child.try_wait(), Ok(None)) {
                return Err(browser_error("浏览器启动超时或已退出"));
            }
            thread::sleep(Duration::from_millis(25));
        }
    }
}

struct Browser {
    socket: WebSocket<TcpStream>,
    next_id: u32,
    deadline: Instant,
    _process: PainterProcess,
}

impl Browser {
    fn open(html: &str, stylesheets: &Value) -> Result<Self, ThemeError> {
        let deadline = Instant::now() + TIMEOUT;
        let mut process = PainterProcess::launch()?;
        let port = process.port(deadline)?;
        let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
        // Explicit loopback transport bypasses inherited OS/HTTP proxy settings.
        let metadata = ureq::Agent::config_builder()
            .proxy(None)
            .max_redirects(0)
            .timeout_global(Some(Duration::from_secs(2)))
            .build()
            .new_agent()
            .get(format!("http://{address}/json/list"))
            .call()
            .ok()
            .and_then(|mut response| response.body_mut().read_to_string().ok())
            .and_then(|body| serde_json::from_str::<Value>(&body).ok())
            .ok_or_else(|| browser_error("无法读取本机浏览器调试端点"))?;
        let path = metadata
            .as_array()
            .into_iter()
            .flatten()
            .find(|target| target["type"] == "page" && target["url"] == "about:blank")
            .and_then(|target| target["id"].as_str())
            .filter(|id| {
                !id.is_empty() && id.len() <= 64 && id.bytes().all(|byte| byte.is_ascii_hexdigit())
            })
            .ok_or_else(|| browser_error("隔离的浏览器没有空白页面"))?;
        let stream = TcpStream::connect_timeout(&address, Duration::from_secs(2))
            .map_err(|_| browser_error("无法连接隔离的浏览器"))?;
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .map_err(protocol_error)?;
        stream
            .set_write_timeout(Some(Duration::from_secs(2)))
            .map_err(protocol_error)?;
        let (socket, _) = client_with_config(
            format!("ws://{address}/devtools/page/{path}"),
            stream,
            Some(WebSocketConfig::default().max_message_size(Some(64 * 1024 * 1024))),
        )
        .map_err(|_| browser_error("无法握手隔离的浏览器"))?;
        let mut browser = Self {
            socket,
            next_id: 0,
            deadline,
            _process: process,
        };
        browser.call("Network.enable", json!({}))?;
        browser.call("Network.setBlockedURLs", json!({
            "urls": ["http:*", "https:*", "file:*", "ftp:*", "ws:*", "wss:*", "chrome:*", "chrome-extension:*"]
        }))?;
        browser.call("Emulation.setDeviceMetricsOverride", json!({
            "width": VIEWPORT_WIDTH, "height": VIEWPORT_HEIGHT, "deviceScaleFactor": 1, "mobile": false
        }))?;
        // The only evaluated browser script is this fixed painter. The rendered
        // HTML is a string, parsed inertly, then active markup is removed.
        let input = serde_json::to_string(html).expect("HTML encodes as a JS string");
        let expression = format!(
            r#"(() => {{
            const parsed = new DOMParser().parseFromString({input}, "text/html");
            const stylesheets = {stylesheets};
            for (const link of parsed.querySelectorAll("link[rel~='stylesheet'][href]")) {{
                const url = link.getAttribute("href");
                if (!stylesheets || !Object.hasOwn(stylesheets, url)) continue;
                const style = parsed.createElement("style");
                if (link.media) style.media = link.media;
                style.textContent = stylesheets[url];
                link.replaceWith(style);
            }}
            parsed.querySelectorAll("script,iframe,frame,frameset,object,embed,portal,base,link,meta[http-equiv],form,svg animate,svg set").forEach(el => el.remove());
            // A blocked remote avatar should leave no broken-image badge.
            for (const source of parsed.querySelectorAll("picture source")) source.remove();
            for (const image of Array.from(parsed.images)) {{
                const src = image.getAttribute("src") || "";
                if (!/^data:image\//i.test(src.trim())) {{
                    image.remove();
                }} else {{
                    image.removeAttribute("srcset");
                    image.loading = "eager";
                }}
            }}
            for (const link of parsed.querySelectorAll("a[href], area[href]")) {{
                if (!/^(?:https?:|mailto:|tel:|#)/i.test(link.getAttribute("href").trim())) {{
                    link.removeAttribute("href");
                }}
            }}
            for (const el of parsed.querySelectorAll("*")) {{
                for (const attr of Array.from(el.attributes)) {{
                    if (/^on/i.test(attr.name)) el.removeAttribute(attr.name);
                }}
            }}
            const policy = parsed.createElement("meta");
            policy.httpEquiv = "Content-Security-Policy";
            policy.content = "default-src 'none'; style-src 'unsafe-inline'; img-src data:; font-src data:; base-uri 'none'; form-action 'none'";
            parsed.head.prepend(policy);
            document.documentElement.replaceWith(document.adoptNode(parsed.documentElement));
        }})()"#
        );
        let reply = browser.call(
            "Runtime.evaluate",
            json!({"expression":expression, "timeout":2500}),
        )?;
        if reply.get("exceptionDetails").is_some() {
            return Err(browser_error("浏览器无法处理主题 HTML"));
        }
        browser.call(
            "Emulation.setScriptExecutionDisabled",
            json!({"value":true}),
        )?;
        Ok(browser)
    }

    fn call(&mut self, method: &str, params: Value) -> Result<Value, ThemeError> {
        self.next_id += 1;
        let id = self.next_id;
        self.socket
            .send(Message::Text(
                json!({"id":id,"method":method,"params":params})
                    .to_string()
                    .into(),
            ))
            .map_err(|_| browser_error("无法向本机浏览器发送绘制请求"))?;
        loop {
            let remaining = self.deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(browser_error("本机浏览器绘制超时"));
            }
            self.socket
                .get_ref()
                .set_read_timeout(Some(remaining))
                .map_err(protocol_error)?;
            let response = self
                .socket
                .read()
                .map_err(|_| browser_error("本机浏览器绘制超时或连接已关闭"))?;
            let Message::Text(body) = response else {
                continue;
            };
            let mut payload: Value =
                serde_json::from_str(&body).map_err(|_| browser_error("本机浏览器响应无效"))?;
            if payload["id"].as_u64() == Some(u64::from(id)) {
                return payload
                    .get_mut("result")
                    .map(Value::take)
                    .ok_or_else(|| browser_error("本机浏览器无法绘制该主题"));
            }
        }
    }
}

fn protocol_error(_: io::Error) -> ThemeError {
    browser_error("无法配置本机浏览器的连接超时")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires a local Chromium-family browser"]
    fn browser_paints_and_prints_without_loading_external_resources() {
        use std::net::TcpListener;

        let trap = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        trap.set_nonblocking(true).unwrap();
        let host = trap.local_addr().unwrap();
        let render = format!(
            r#"globalThis.__resumeOneTheme = {{
          render(resume) {{
            return `<html><head><style>
            body {{ color: #814b2e; }}
            @import url(http://{host}/styles);
            </style><meta http-equiv="refresh" content="0; url=http://{host}/refresh"></head>
            <body><h1>${{resume.basics.name}}</h1><img src="http://{host}/private"
            onerror="document.body.innerText = 'script ran'">
            <script>document.body.innerText = 'script ran'</script></body></html>`;
          }}
        }}"#
        );
        let theme = OriginalTheme::parse(&crate::original::tests::fixture(&render)).unwrap();
        let resume = json!({"basics":{"name":"Private fixture"}});
        let preview = preview_original(&theme, &resume).unwrap();
        assert_eq!(preview.width, VIEWPORT_WIDTH);
        assert_eq!(preview.strips[0].0, VIEWPORT_HEIGHT);
        assert!(preview.strips[0].1.starts_with(b"\x89PNG\r\n\x1a\n"));
        assert!(
            print_original(&theme, &resume)
                .unwrap()
                .starts_with(b"%PDF-")
        );
        assert_eq!(trap.accept().unwrap_err().kind(), io::ErrorKind::WouldBlock);
    }

    #[test]
    #[ignore = "requires local Chromium and the theme repository checkout"]
    fn original_catalog_paints_every_pinned_theme() {
        let index = PathBuf::from(
            std::env::var_os("RESUME_ONE_ORIGINAL_CATALOG").expect("path to original index"),
        );
        let catalog = crate::Catalog::parse(
            crate::GithubSource::community_default(),
            &"0".repeat(40),
            &fs::read(&index).unwrap(),
        )
        .unwrap();
        let resume =
            serde_json::from_slice(include_bytes!("../../../assets/example.resume.json")).unwrap();
        assert!(!catalog.entries().is_empty());
        for entry in catalog.entries() {
            let bytes = fs::read(index.parent().unwrap().join(&entry.manifest)).unwrap();
            let verified = crate::VerifiedTheme::parse(entry, &bytes).unwrap();
            let preview = preview_original(verified.original().unwrap(), &resume)
                .unwrap_or_else(|error| panic!("{}: {error}", entry.id));
            assert_eq!(preview.width, VIEWPORT_WIDTH);
            assert!(preview.strips[0].1.starts_with(b"\x89PNG\r\n\x1a\n"));
        }
    }

    #[test]
    fn respects_upstream_print_geometry_and_caps_values() {
        let options = print_options(&RenderedHtml {
            html: String::new(),
            print: json!({
                "format":"Letter", "margin":{"top":"1cm","bottom":"999in"}, "scale":1.25
            }),
        });
        assert_eq!(options["paperWidth"], 8.5);
        assert_eq!(options["marginTop"], 1. / 2.54);
        assert_eq!(options["marginBottom"], 0.);
        assert_eq!(options["scale"], 1.25);
    }
}
