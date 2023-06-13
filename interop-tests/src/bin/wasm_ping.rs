use std::env;
use std::process::Stdio;
use std::time::Duration;

use anyhow::{Context, Result};
use axum::body;
use axum::http::{header, Uri};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use axum::{extract::State, http::StatusCode, routing::post, Json, Router};
use redis::{AsyncCommands, Client};
use thirtyfour::prelude::*;
use tokio::io::{AsyncBufReadExt, BufReader};
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tracing::warn;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

use interop_tests::BlpopRequest;

mod config;

/// Embedded Wasm package
///
/// Make sure to build the wasm with `wasm-pack build --target web`
#[derive(rust_embed::RustEmbed)]
#[folder = "pkg"]
struct WasmPackage;

#[tokio::main]
async fn main() -> Result<()> {
    // start logging
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::from_default_env())
        .init();

    // read env variables
    let config = config::Config::from_env()?;
    let test_timeout = Duration::from_secs(config.test_timeout);
    let proxy_addr = env::var("proxy_addr").unwrap_or_else(|_| "127.0.0.1:6378".into());
    let proxy_addr_clone = proxy_addr.clone();

    // create a redis client
    let client = Client::open(config.redis_addr.as_str()).context("Could not connect to redis")?;

    // create a wasm-app service
    let app = Router::new()
        // Redis proxy
        .route("/blpop", post(redis_blpop))
        // Wasm ping test trigger
        .route("/", get(serve_index_html))
        .route("/index.html", get(serve_index_html))
        .route(
            "/index.js",
            get(|| async move { serve_index_js(&config, &proxy_addr_clone).await }),
        )
        // Wasm app static files
        .fallback(serve_wasm_pkg)
        // Middleware
        .layer(CorsLayer::very_permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(client);

    // Run the service in background
    tokio::spawn(axum::Server::bind(&proxy_addr.parse()?).serve(app.into_make_service()));

    // Execute the the test with a webdriver
    run_test(&proxy_addr, test_timeout).await
}

async fn run_test(proxy_addr: &str, timeout: Duration) -> Result<()> {
    // start a webdriver process
    // currently only the chromedriver is supported as firefox doesn't
    // have support yet for the certhashes
    let mut driver = tokio::process::Command::new("chromedriver")
        .arg("--port=45782")
        .stdout(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    // read driver's stdout
    let driver_out = driver
        .stdout
        .take()
        .context("No stdout found for webdriver")?;
    // wait for the 'ready' message
    let mut reader = BufReader::new(driver_out).lines();
    while let Some(line) = reader.next_line().await? {
        if line.contains("ChromeDriver was started successfully.") {
            break;
        }
    }

    // run a webdriver client
    let mut caps = DesiredCapabilities::chrome();
    caps.set_headless()?;
    let driver = WebDriver::new("http://localhost:45782", caps).await?;
    // go to the wasm test service
    driver.goto(format!("http://{}", proxy_addr)).await?;
    // wait for the script to finish and set the result
    match driver
        .query(By::Id("result"))
        .wait(timeout, Duration::from_millis(200))
        .first()
        .await
    {
        // print the result
        Ok(span) => {
            println!("{}", span.text().await?);
            driver.quit().await?;
            Ok(())
        }
        // or return a timeout error
        Err(e) => {
            driver.quit().await?;
            Err(e).context("Timed out waiting for the test result")
        }
    }
}

/// Redis proxy handler.
/// `blpop` is currently the only redis client method used in a ping dialer.
async fn redis_blpop(
    State(client): State<Client>,
    request: Json<BlpopRequest>,
) -> Result<Json<Vec<String>>, StatusCode> {
    let mut conn = client.get_async_connection().await.map_err(|e| {
        warn!("Failed to connect to redis: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    let res = conn
        .blpop(&request.key, request.timeout as usize)
        .await
        .map_err(|e| {
            warn!(
                "Failed to get list elem {} within timeout {}: {e}",
                request.key, request.timeout
            );
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    Ok(Json(res))
}

/// Serve the main page which loads our javascript
async fn serve_index_html() -> Result<impl IntoResponse, StatusCode> {
    let html = r#"
        <!DOCTYPE html>
        <html>
        <head>
            <meta charset="UTF-8" />
            <title>libp2p ping test</title>
            <script type="module" src="./index.js"></script>
        </head>

        <body></body>
        </html>
    "#;

    Ok(Html(html))
}

/// Serve a js script which runs the main test function
async fn serve_index_js(
    config: &config::Config,
    redis_proxy_addr: &str,
) -> Result<impl IntoResponse, StatusCode> {
    // create the script
    let script = format!(
        r#"
            // import a wasm initialization fn and our test entrypoint
            import init, {{ run_test_wasm }} from "/interop_tests.js";

            const runWasm = async () => {{
                // initialize wasm
                let res = await init()
                    // run our entrypoint with params from the env
                    .then(() => run_test_wasm(
                        "{}",
                        "{}",
                        {},
                        "{}",
                        "{redis_proxy_addr}"
                    ))
                    // handle the `Err` variant
                    .catch(e => `${{e}}`);

                // update the body with the result span
                document.body.innerHTML = `<span id="result">${{res}}<span>`;
            }};

            runWasm();
        "#,
        config.transport, config.ip, config.is_dialer, config.test_timeout,
    );

    Ok(([(header::CONTENT_TYPE, "application/javascript")], script))
}

async fn serve_wasm_pkg(uri: Uri) -> Result<Response, StatusCode> {
    let path = uri.path().trim_start_matches('/').to_string();
    if let Some(content) = WasmPackage::get(&path) {
        let mime = mime_guess::from_path(&path).first_or_octet_stream();
        Ok(Response::builder()
            .header(header::CONTENT_TYPE, mime.as_ref())
            .body(body::boxed(body::Full::from(content.data)))
            .unwrap())
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}
