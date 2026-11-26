//! Request bodies that are not JSON: an HTML form (`application/x-www-form-urlencoded`) and `multipart/form-data`
//! (a form with files). The server reads them once; the text fields fill the payload of the endpoint, as a query
//! string does, and the files go to the request.

/// A file of a `multipart/form-data` body.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Upload {
    /// The name of the field.
    pub name: String,
    /// The name of the file, as the client sent it.
    pub filename: Option<String>,
    pub content_type: Option<String>,
    pub data: Vec<u8>,
}

/// The text fields and the files of a form body.
pub type Form = (Vec<(String, String)>, Vec<Upload>);

/// `a=1&b=x+y&c=%2F`: the pairs in order, with `+` as a space and the percent escapes read.
pub fn parse_urlencoded(body: &[u8]) -> Result<Vec<(String, String)>, String> {
    let text = std::str::from_utf8(body).map_err(|_| "the form is not UTF-8 text".to_string())?;
    let mut out = Vec::new();
    for pair in text.split('&').filter(|p| !p.is_empty()) {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        out.push((decode(k)?, decode(v)?));
    }
    Ok(out)
}

fn decode(text: &str) -> Result<String, String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' => {
                let hex = bytes
                    .get(i + 1..i + 3)
                    .and_then(|h| std::str::from_utf8(h).ok())
                    .and_then(|h| u8::from_str_radix(h, 16).ok())
                    .ok_or_else(|| format!("the form has a bad escape in `{text}`"))?;
                out.push(hex);
                i += 2;
            }
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8(out).map_err(|_| format!("the form field `{text}` is not UTF-8 text"))
}

/// The boundary of a `multipart/form-data` content type, quoted or not.
fn boundary(content_type: &str) -> Option<String> {
    content_type.split(';').skip(1).find_map(|p| {
        let (k, v) = p.trim().split_once('=')?;
        k.trim()
            .eq_ignore_ascii_case("boundary")
            .then(|| v.trim().trim_matches('"').to_string())
            .filter(|b| !b.is_empty())
    })
}

/// A parameter of a `content-disposition` line: `name="x"`.
fn disposition_param(line: &str, key: &str) -> Option<String> {
    line.split(';').skip(1).find_map(|p| {
        let (k, v) = p.trim().split_once('=')?;
        k.trim()
            .eq_ignore_ascii_case(key)
            .then(|| v.trim().trim_matches('"').to_string())
    })
}

fn find(haystack: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    haystack
        .get(from..)?
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|p| p + from)
}

/// The text fields and the files of a `multipart/form-data` body.
pub fn parse_multipart(body: &[u8], content_type: &str) -> Result<Form, String> {
    let boundary = boundary(content_type)
        .ok_or_else(|| "the multipart body has no boundary in its content type".to_string())?;
    let delimiter = format!("--{boundary}").into_bytes();
    let mut at = find(body, &delimiter, 0)
        .ok_or_else(|| "the multipart body does not start with its boundary".to_string())?;
    let (mut fields, mut files) = (Vec::new(), Vec::new());
    loop {
        at += delimiter.len();
        if body.get(at..at + 2) == Some(b"--") {
            return Ok((fields, files));
        }
        // The line break after the boundary.
        if body.get(at..at + 2) != Some(b"\r\n") {
            return Err("the multipart body is cut or malformed".into());
        }
        at += 2;
        let head_end = find(body, b"\r\n\r\n", at)
            .ok_or_else(|| "a part of the multipart body has no end of headers".to_string())?;
        let head = String::from_utf8_lossy(&body[at..head_end]).to_string();
        let start = head_end + 4;
        let mut closing = b"\r\n".to_vec();
        closing.extend_from_slice(&delimiter);
        let end = find(body, &closing, start).ok_or_else(|| {
            "a part of the multipart body is not closed by the boundary".to_string()
        })?;
        let data = body[start..end].to_vec();
        let (mut name, mut filename, mut kind) = (None, None, None);
        for line in head.split("\r\n") {
            let Some((k, v)) = line.split_once(':') else {
                continue;
            };
            if k.trim().eq_ignore_ascii_case("content-disposition") {
                name = disposition_param(v, "name");
                filename = disposition_param(v, "filename");
            } else if k.trim().eq_ignore_ascii_case("content-type") {
                kind = Some(v.trim().to_string());
            }
        }
        let name = name.ok_or_else(|| "a part of the multipart body has no name".to_string())?;
        if filename.is_some() {
            files.push(Upload {
                name,
                filename,
                content_type: kind,
                data,
            });
        } else {
            fields.push((name, String::from_utf8_lossy(&data).into_owned()));
        }
        at = end + 2;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_form_is_read_with_its_escapes() {
        assert_eq!(
            parse_urlencoded(b"name=Ana+Maria&city=S%C3%A3o+Paulo&empty=&flag").unwrap(),
            [
                ("name".into(), "Ana Maria".into()),
                ("city".into(), "São Paulo".into()),
                ("empty".into(), String::new()),
                ("flag".into(), String::new()),
            ]
        );
        assert_eq!(
            parse_urlencoded(b"").unwrap(),
            Vec::<(String, String)>::new()
        );
        assert!(
            parse_urlencoded(b"a=%zz")
                .unwrap_err()
                .contains("bad escape")
        );
        assert!(
            parse_urlencoded(b"a=%C3")
                .unwrap_err()
                .contains("not UTF-8")
        );
    }

    #[test]
    fn multipart_has_fields_and_files() {
        let body = b"--XyZ\r\n\
Content-Disposition: form-data; name=\"title\"\r\n\r\n\
My photo\r\n\
--XyZ\r\n\
Content-Disposition: form-data; name=\"photo\"; filename=\"a.png\"\r\n\
Content-Type: image/png\r\n\r\n\
\x89PNG\r\n\x00\xff\r\n\
--XyZ\r\n\
Content-Disposition: form-data; name=\"notes\"; filename=\"n.txt\"\r\n\r\n\
\r\n\
--XyZ--\r\n";
        let (fields, files) = parse_multipart(body, "multipart/form-data; boundary=XyZ").unwrap();
        assert_eq!(fields, [("title".to_string(), "My photo".to_string())]);
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].name, "photo");
        assert_eq!(files[0].filename.as_deref(), Some("a.png"));
        assert_eq!(files[0].content_type.as_deref(), Some("image/png"));
        // The bytes as sent, a line break and a NUL inside them included.
        assert_eq!(files[0].data, b"\x89PNG\r\n\x00\xff");
        assert_eq!(
            (files[1].filename.as_deref(), files[1].data.len()),
            (Some("n.txt"), 0)
        );
        // A quoted boundary.
        let quoted = b"--a b\r\nContent-Disposition: form-data; name=\"x\"\r\n\r\n1\r\n--a b--";
        assert_eq!(
            parse_multipart(quoted, "multipart/form-data; boundary=\"a b\"")
                .unwrap()
                .0,
            [("x".to_string(), "1".to_string())]
        );
    }

    #[test]
    fn a_broken_multipart_body_is_an_error() {
        let ct = "multipart/form-data; boundary=B";
        assert!(
            parse_multipart(b"x", "multipart/form-data")
                .unwrap_err()
                .contains("no boundary")
        );
        assert!(
            parse_multipart(b"nothing", ct)
                .unwrap_err()
                .contains("does not start")
        );
        assert!(
            parse_multipart(
                b"--B\r\nContent-Disposition: form-data; name=\"a\"\r\n\r\nno end",
                ct
            )
            .unwrap_err()
            .contains("not closed")
        );
        assert!(
            parse_multipart(b"--B\r\nContent-Type: text/plain\r\n\r\nx\r\n--B--", ct)
                .unwrap_err()
                .contains("no name")
        );
    }
}
