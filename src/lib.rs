//! Notes in an S3 bucket, through Cosmonic Desktop's `assets` binding.
//!
//! ```text
//! GET    /                    the web UI
//! GET    /api/notes           {"bucket","notes":[{"name","size"}]}
//! GET    /api/notes/<name>    the note's text
//! PUT    /api/notes/<name>    store the request body as the note
//! DELETE /api/notes/<name>    delete it
//! ```
//!
//! Each note is one object in the bucket `cosmonic-notes`, which must already
//! exist (S3 buckets are provisioned outside wasmCloud; the sample's
//! `scripts/start-s3.sh` creates it). The component never sees the endpoint
//! or the access keys: those belong to the host's binding.

mod bindings {
    wit_bindgen::generate!({ generate_all });
}

use bindings::exports::wasi::http::handler::Guest as Handler;
use bindings::wasi::http::types::{ErrorCode, Fields, Method, Request, Response};
use bindings::wasmcloud::blobstore::types::Error as BlobError;
use serde_json::json;

struct Component;

const BUCKET: &str = "cosmonic-notes";
/// A note is small text; refuse anything bigger rather than buffer it.
const MAX_NOTE_BYTES: usize = 1024 * 1024;
const INDEX_HTML: &str = include_str!("../ui/index.html");

struct Reply {
    status: u16,
    content_type: &'static str,
    body: Vec<u8>,
}

fn json_reply(status: u16, value: serde_json::Value) -> Reply {
    Reply {
        status,
        content_type: "application/json",
        body: value.to_string().into_bytes(),
    }
}

fn error(status: u16, message: impl Into<String>) -> Reply {
    json_reply(status, json!({ "error": message.into() }))
}

fn blob_error(what: &str, e: BlobError) -> Reply {
    match e {
        BlobError::NoSuchObject => error(404, format!("{what}: no such note")),
        BlobError::NoSuchContainer => error(
            503,
            format!("{what}: the bucket {BUCKET} does not exist; create it (scripts/start-s3.sh does)"),
        ),
        other => error(502, format!("{what}: {other:?}")),
    }
}

/// A note name: a single safe S3 key segment.
fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && !name.starts_with('.')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

fn respond(reply: Reply) -> Result<Response, ErrorCode> {
    let headers = Fields::from_list(&[
        ("content-type".to_string(), reply.content_type.as_bytes().to_vec()),
        ("cache-control".to_string(), b"no-store".to_vec()),
    ])
    .map_err(|e| ErrorCode::InternalError(Some(format!("{e:?}"))))?;
    let (mut tx, rx) = bindings::wit_stream::new();
    let (trailers_tx, trailers_rx) = bindings::wit_future::new(|| Ok(None));
    let body = reply.body;
    wit_bindgen::spawn_local(async move {
        tx.write_all(body).await;
        drop(tx);
        let _ = trailers_tx.write(Ok(None)).await;
    });
    let (response, _done) = Response::new(headers, Some(rx), trailers_rx);
    let _ = response.set_status_code(reply.status);
    Ok(response)
}

async fn route(request: Request) -> Reply {
    let method = request.get_method();
    let full = request.get_path_with_query().unwrap_or_else(|| "/".into());
    let path = full.split('?').next().unwrap_or("/").to_string();
    if path == "/" || path == "/index.html" {
        return Reply {
            status: 200,
            content_type: "text/html; charset=utf-8",
            body: INDEX_HTML.as_bytes().to_vec(),
        };
    }
    let segments: Vec<&str> = path.trim_matches('/').split('/').collect();
    let name = match segments.as_slice() {
        ["api", "notes"] => None,
        ["api", "notes", name] => Some(name.to_string()),
        _ => return error(404, format!("no route for {path}")),
    };
    if let Some(n) = &name {
        if !valid_name(n) {
            return error(400, "a note name is letters, digits, '-', '_' or '.', up to 128");
        }
    }

    let container = match bindings::assets::get_container(BUCKET.to_string()).await {
        Ok(c) => c,
        Err(e) => return blob_error("open the bucket", e),
    };

    match (method, name) {
        (Method::Get, None) => {
            let names = match container.list_objects().await {
                Ok(stream) => stream.collect().await,
                Err(e) => return blob_error("list notes", e),
            };
            let mut notes = Vec::new();
            for n in names {
                let size = match container.object_info(n.clone()).await {
                    Ok(info) => info.size,
                    Err(_) => 0,
                };
                notes.push(json!({ "name": n, "size": size }));
            }
            notes.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
            json_reply(200, json!({ "bucket": BUCKET, "notes": notes }))
        }
        (Method::Get, Some(n)) => match container.get_data(n.clone(), 0, u64::MAX).await {
            Ok(stream) => Reply {
                status: 200,
                content_type: "text/plain; charset=utf-8",
                body: stream.collect().await,
            },
            Err(e) => blob_error(&format!("read {n}"), e),
        },
        (Method::Put | Method::Post, Some(n)) => {
            let (_res_tx, res_rx) = bindings::wit_future::new(|| Ok(()));
            let (body, _trailers) = Request::consume_body(request, res_rx);
            let data = body.collect().await;
            if data.len() > MAX_NOTE_BYTES {
                return error(413, format!("a note is at most {MAX_NOTE_BYTES} bytes"));
            }
            let len = data.len();
            let (mut tx, rx) = bindings::wit_stream::new();
            wit_bindgen::spawn_local(async move {
                tx.write_all(data).await;
                drop(tx);
            });
            match container.write_data(n.clone(), rx).await {
                Ok(()) => json_reply(201, json!({ "name": n, "size": len })),
                Err(e) => blob_error(&format!("save {n}"), e),
            }
        }
        (Method::Delete, Some(n)) => match container.delete_object(n.clone()).await {
            Ok(()) => json_reply(200, json!({ "deleted": n })),
            Err(e) => blob_error(&format!("delete {n}"), e),
        },
        _ => error(405, "method not allowed"),
    }
}

impl Handler for Component {
    async fn handle(request: Request) -> Result<Response, ErrorCode> {
        respond(route(request).await)
    }
}

bindings::export!(Component with_types_in bindings);
