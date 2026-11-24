//! GET HTTPS mínimo con WinHTTP (nativo de Windows, sin dependencias externas).

use windows::core::{w, HSTRING, PCWSTR};
use windows::Win32::Networking::WinHttp::*;

pub struct Response {
    pub status: u32,
    pub body: String,
}

struct Handle(*mut core::ffi::c_void);
impl Drop for Handle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                let _ = WinHttpCloseHandle(self.0);
            }
        }
    }
}

/// `host` sin esquema (ej. "lrclib.net"), `path` con query ya codificada.
pub fn get(host: &str, path: &str) -> Option<Response> {
    get_with_headers(host, path, "")
}

/// Igual que `get`, con encabezados extra (cada línea terminada en CRLF), p. ej. una API key.
pub fn get_with_headers(host: &str, path: &str, extra_headers: &str) -> Option<Response> {
    unsafe {
        let session = Handle(WinHttpOpen(
            w!("Esme/0.5 (+https://github.com/Maurito2004/Esme)"),
            WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
            PCWSTR::null(),
            PCWSTR::null(),
            0,
        ));
        if session.0.is_null() {
            return None;
        }
        let _ = WinHttpSetTimeouts(session.0, 5000, 5000, 5000, 8000);
        let conn = Handle(WinHttpConnect(session.0, &HSTRING::from(host), INTERNET_DEFAULT_HTTPS_PORT, 0));
        if conn.0.is_null() {
            return None;
        }
        let req = Handle(WinHttpOpenRequest(
            conn.0,
            w!("GET"),
            &HSTRING::from(path),
            PCWSTR::null(),
            PCWSTR::null(),
            std::ptr::null_mut(),
            WINHTTP_FLAG_SECURE,
        ));
        if req.0.is_null() {
            return None;
        }
        let headers: Vec<u16> = extra_headers.encode_utf16().collect();
        let headers = (!headers.is_empty()).then_some(headers.as_slice());
        WinHttpSendRequest(req.0, headers, None, 0, 0, 0).ok()?;
        WinHttpReceiveResponse(req.0, std::ptr::null_mut()).ok()?;

        let mut status = 0u32;
        let mut len = 4u32;
        WinHttpQueryHeaders(
            req.0,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            PCWSTR::null(),
            Some(&mut status as *mut u32 as *mut _),
            &mut len,
            std::ptr::null_mut(),
        )
        .ok()?;

        let mut body = Vec::new();
        let mut buf = vec![0u8; 16 * 1024];
        loop {
            let mut read = 0u32;
            WinHttpReadData(req.0, buf.as_mut_ptr() as *mut _, buf.len() as u32, &mut read).ok()?;
            if read == 0 {
                break;
            }
            body.extend_from_slice(&buf[..read as usize]);
            if body.len() > 16 * 1024 * 1024 {
                return None; // respuesta absurda: cortamos
            }
        }
        Some(Response { status, body: String::from_utf8_lossy(&body).into_owned() })
    }
}

/// POST HTTPS con payload JSON usando WinHTTP nativo.
/// `extra_headers` va tal cual (cada línea terminada en "\r\n"), p. ej. la API key,
/// para que nunca viaje en la URL.
pub fn post_json(host: &str, path: &str, extra_headers: &str, json_body: &str, recv_timeout_ms: i32) -> Option<Response> {
    unsafe {
        let session = Handle(WinHttpOpen(
            w!("Esme/0.6 (+https://github.com/Maurito2004/Esme)"),
            WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
            PCWSTR::null(),
            PCWSTR::null(),
            0,
        ));
        if session.0.is_null() {
            return None;
        }
        // Recepción larga: los modelos que "piensan" pueden tardar bastante en responder.
        let _ = WinHttpSetTimeouts(session.0, 5000, 5000, 8000, recv_timeout_ms);
        let conn = Handle(WinHttpConnect(session.0, &HSTRING::from(host), INTERNET_DEFAULT_HTTPS_PORT, 0));
        if conn.0.is_null() {
            return None;
        }
        let req = Handle(WinHttpOpenRequest(
            conn.0,
            w!("POST"),
            &HSTRING::from(path),
            PCWSTR::null(),
            PCWSTR::null(),
            std::ptr::null_mut(),
            WINHTTP_FLAG_SECURE,
        ));
        if req.0.is_null() {
            return None;
        }
        let headers: Vec<u16> = format!("Content-Type: application/json; charset=utf-8\r\n{extra_headers}")
            .encode_utf16()
            .collect();
        let headers_slice = headers.as_slice();
        let bytes = json_body.as_bytes();
        WinHttpSendRequest(
            req.0,
            Some(headers_slice),
            Some(bytes.as_ptr() as *const _),
            bytes.len() as u32,
            bytes.len() as u32,
            0,
        )
        .ok()?;
        WinHttpReceiveResponse(req.0, std::ptr::null_mut()).ok()?;

        let mut status = 0u32;
        let mut len = 4u32;
        WinHttpQueryHeaders(
            req.0,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            PCWSTR::null(),
            Some(&mut status as *mut u32 as *mut _),
            &mut len,
            std::ptr::null_mut(),
        )
        .ok()?;

        let mut body = Vec::new();
        let mut buf = vec![0u8; 16 * 1024];
        loop {
            let mut read = 0u32;
            WinHttpReadData(req.0, buf.as_mut_ptr() as *mut _, buf.len() as u32, &mut read).ok()?;
            if read == 0 {
                break;
            }
            body.extend_from_slice(&buf[..read as usize]);
            if body.len() > 16 * 1024 * 1024 {
                return None;
            }
        }
        Some(Response { status, body: String::from_utf8_lossy(&body).into_owned() })
    }
}

/// Codificación de URL (RFC 3986) para valores de query.
pub fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}
