//! An OpenAPI 3.0 document built from the routing table, and the page that shows it.

use serde_json::{Map, Value, json};
use telvax_hir::Prim;
use telvax_hir::hir::HttpMethod;

use crate::layers::ErrorFormat;
use crate::{Kind, Route, Schema};

/// The OpenAPI document of `routes`: paths, parameters, bodies and the statuses the server answers.
pub fn document(title: &str, routes: &[Route]) -> Value {
    document_with(title, routes, ErrorFormat::Telvax)
}

/// Like [`document`], with the errors written in `format`.
pub fn document_with(title: &str, routes: &[Route], format: ErrorFormat) -> Value {
    let mut paths = Map::new();
    for route in routes {
        let item = paths
            .entry(route.path.clone())
            .or_insert_with(|| json!({}))
            .as_object_mut()
            .expect("an object");
        item.insert(method_name(route.method).into(), operation(route, format));
    }
    json!({
        "openapi": "3.0.3",
        "info": { "title": title, "version": "1.0.0" },
        "paths": paths,
        "components": { "schemas": { "Error": error_schema(format) } },
    })
}

fn method_name(m: HttpMethod) -> &'static str {
    match m {
        HttpMethod::Get => "get",
        HttpMethod::Post => "post",
        HttpMethod::Put => "put",
        HttpMethod::Patch => "patch",
        HttpMethod::Delete => "delete",
    }
}

fn error_schema(format: ErrorFormat) -> Value {
    if format == ErrorFormat::Detail {
        return json!({
            "type": "object",
            "properties": { "detail": { "oneOf": [
                { "type": "string" },
                { "type": "array", "items": { "type": "string" } },
            ] } },
            "required": ["detail"],
        });
    }
    json!({
        "type": "object",
        "properties": { "error": { "type": "string" } },
        "required": ["error"],
    })
}

fn error_response(description: &str) -> Value {
    json!({
        "description": description,
        "content": { "application/json": { "schema": { "$ref": "#/components/schemas/Error" } } },
    })
}

fn operation(route: &Route, format: ErrorFormat) -> Value {
    let mut parameters: Vec<Value> = route
        .path_params
        .iter()
        .map(|(name, kind)| {
            json!({ "name": name, "in": "path", "required": true, "schema": schema(kind) })
        })
        .collect();
    let mut op = Map::new();
    op.insert("operationId".into(), json!(route.name));
    let reads_query = matches!(route.method, HttpMethod::Get | HttpMethod::Delete);
    if let Some(payload) = &route.payload {
        if reads_query {
            parameters.extend(payload.fields.iter().map(|(name, kind)| {
                let required = !payload.defaults.iter().any(|(n, _)| n == name);
                json!({ "name": name, "in": "query", "required": required, "schema": schema(kind) })
            }));
        } else {
            op.insert(
                "requestBody".into(),
                json!({
                    "required": true,
                    "content": { "application/json": { "schema": object(payload) } },
                }),
            );
        }
    }
    if !parameters.is_empty() {
        op.insert("parameters".into(), Value::Array(parameters));
    }
    let ok = match &route.response {
        Some(kind) => json!({
            "description": "The value the function returns.",
            "content": { "application/json": { "schema": schema(kind) } },
        }),
        None => json!({ "description": "The answer." }),
    };
    let mut responses = Map::new();
    responses.insert("200".into(), ok);
    if route.payload.is_some() || !route.path_params.is_empty() {
        let status = if format == ErrorFormat::Detail {
            "422"
        } else {
            "400"
        };
        responses.insert(
            status.into(),
            error_response("The request does not fit the types."),
        );
    }
    responses.insert(
        "default".into(),
        error_response("The status and message of an `Err(Status...)`, or a 500."),
    );
    op.insert("responses".into(), Value::Object(responses));
    Value::Object(op)
}

fn object(s: &Schema) -> Value {
    let properties: Map<String, Value> = s
        .fields
        .iter()
        .map(|(name, kind)| {
            let mut property = schema(kind);
            match s.defaults.iter().find(|(n, _)| n == name) {
                Some((_, d)) if !d.is_null() => {
                    if let Some(p) = property.as_object_mut() {
                        p.insert("default".into(), d.clone());
                    }
                }
                _ => {}
            }
            (name.clone(), property)
        })
        .collect();
    let required: Vec<&String> = s
        .fields
        .iter()
        .map(|(name, _)| name)
        .filter(|name| !s.defaults.iter().any(|(n, _)| n == *name))
        .collect();
    json!({ "type": "object", "properties": properties, "required": required })
}

/// The JSON schema of a kind.
pub fn schema(kind: &Kind) -> Value {
    match kind {
        Kind::Int(p) => {
            let format = match p {
                Prim::I8 | Prim::I16 | Prim::I32 | Prim::U8 | Prim::U16 => "int32",
                _ => "int64",
            };
            let mut s = json!({ "type": "integer", "format": format });
            if matches!(p, Prim::U8 | Prim::U16 | Prim::U32 | Prim::U64) {
                s["minimum"] = json!(0);
            }
            s
        }
        Kind::Float => json!({ "type": "number" }),
        Kind::Bool => json!({ "type": "boolean" }),
        Kind::Str => json!({ "type": "string" }),
        Kind::Array(inner) => json!({ "type": "array", "items": schema(inner) }),
        Kind::Tensor { elem, shape } => shape.iter().rev().fold(schema(elem), |inner, dim| {
            let mut s = json!({ "type": "array", "items": inner });
            if let Some(n) = dim {
                s["minItems"] = json!(n);
                s["maxItems"] = json!(n);
            }
            s
        }),
        Kind::Object(s) => object(s),
        Kind::Any => json!({}),
    }
}

/// A page that reads `/openapi.json` and lists the routes with their parameters and bodies.
pub const DOCS_PAGE: &str = r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>API</title>
<style>
body{font:15px/1.5 system-ui,sans-serif;max-width:56rem;margin:2rem auto;padding:0 1rem;color:#1b1f24;background:#fff}
@media(prefers-color-scheme:dark){body{color:#e6e8eb;background:#14171a}pre{background:#1e2226}}
h1{font-size:1.4rem}.op{border:1px solid #8884;border-radius:6px;margin:.8rem 0;padding:.6rem .9rem}
.m{display:inline-block;min-width:4.5rem;font-weight:700;text-transform:uppercase}
pre{background:#f4f5f6;padding:.6rem;border-radius:4px;overflow:auto}code{font-family:ui-monospace,monospace}
</style>
</head>
<body>
<h1 id="title">API</h1>
<p><a href="/openapi.json">openapi.json</a></p>
<div id="ops"></div>
<script>
fetch('/openapi.json').then(r => r.json()).then(doc => {
  document.getElementById('title').textContent = doc.info.title;
  const ops = document.getElementById('ops');
  const el = (tag, text, cls) => { const e = document.createElement(tag); if (text) e.textContent = text; if (cls) e.className = cls; return e; };
  for (const [path, item] of Object.entries(doc.paths)) {
    for (const [method, op] of Object.entries(item)) {
      const box = el('div', '', 'op');
      const head = el('div');
      head.append(el('span', method, 'm'), el('code', path));
      box.append(head);
      for (const p of op.parameters || []) box.append(el('div', p.in + ' parameter ' + p.name + ': ' + JSON.stringify(p.schema)));
      const body = op.requestBody && op.requestBody.content['application/json'].schema;
      if (body) { box.append(el('div', 'body')); box.append(el('pre', JSON.stringify(body, null, 2))); }
      for (const [status, r] of Object.entries(op.responses)) {
        const s = r.content && r.content['application/json'].schema;
        box.append(el('div', status + ' ' + r.description));
        if (s && status === '200') box.append(el('pre', JSON.stringify(s, null, 2)));
      }
      ops.append(box);
    }
  }
});
</script>
</body>
</html>
"#;
