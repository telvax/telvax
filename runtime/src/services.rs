//! Real backends for the std AI services: Ollama embeddings, OpenAI-compatible chat completions
//! and pgvector. Enabled with `Interp::set_live(true)` (`telvax serve --live`).

use std::collections::HashSet;
use std::sync::Mutex;
use std::time::Duration;

use serde_json::{Value as Json, json};

use crate::interp::RuntimeError;

fn err<T>(msg: impl Into<String>) -> Result<T, RuntimeError> {
    Err(RuntimeError::new(msg))
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(120)))
        .http_status_as_error(false)
        .build()
        .into()
}

/// Sends `body` as JSON and returns the decoded JSON reply, turning HTTP errors into messages.
fn post_json(url: &str, bearer: Option<&str>, body: &Json) -> Result<Json, RuntimeError> {
    let mut req = agent().post(url);
    if let Some(key) = bearer {
        req = req.header("Authorization", &format!("Bearer {key}"));
    }
    let mut resp = req
        .send_json(body)
        .map_err(|e| RuntimeError::new(format!("request to {url} failed: {e}")))?;
    let status = resp.status().as_u16();
    let reply: Json = resp.body_mut().read_json().map_err(|e| {
        RuntimeError::new(format!(
            "{url} returned an unreadable reply (HTTP {status}): {e}"
        ))
    })?;
    if !(200..300).contains(&status) {
        let detail = reply["error"]["message"]
            .as_str()
            .or_else(|| reply["error"].as_str())
            .unwrap_or("no details");
        return err(format!("{url} answered HTTP {status}: {detail}"));
    }
    Ok(reply)
}

/// `POST {host}/api/embed` (Ollama). The reply must have exactly `dim` numbers.
pub(crate) fn ollama_embed(
    host: &str,
    model: &str,
    text: &str,
    dim: usize,
) -> Result<Vec<f32>, RuntimeError> {
    let reply = post_json(
        &format!("{}/api/embed", host.trim_end_matches('/')),
        None,
        &json!({ "model": model, "input": text }),
    )?;
    let vector = reply["embeddings"][0]
        .as_array()
        .ok_or_else(|| RuntimeError::new("Ollama reply has no `embeddings[0]`"))?;
    let out: Vec<f32> = vector
        .iter()
        .filter_map(|x| x.as_f64())
        .map(|x| x as f32)
        .collect();
    if out.len() != vector.len() {
        return err("Ollama returned non-numeric embedding values");
    }
    if out.len() != dim {
        return err(format!(
            "model `{model}` returned {} dimensions but the type requires {dim}",
            out.len()
        ));
    }
    Ok(out)
}

/// `POST {base}/chat/completions` (OpenAI and compatible servers).
pub(crate) fn openai_complete(
    base: &str,
    key: &str,
    model: &str,
    prompt: &str,
) -> Result<String, RuntimeError> {
    let reply = post_json(
        &format!("{}/chat/completions", base.trim_end_matches('/')),
        Some(key),
        &json!({ "model": model, "messages": [{ "role": "user", "content": prompt }] }),
    )?;
    reply["choices"][0]["message"]["content"]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| RuntimeError::new("completion reply has no `choices[0].message.content`"))
}

/// One pgvector connection; each collection is a table `(id, text, embedding vector(dim))`.
pub struct PgStore {
    client: Mutex<postgres::Client>,
    ready: Mutex<HashSet<String>>,
    dim: usize,
}

impl std::fmt::Debug for PgStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PgStore")
    }
}

/// Collection names become table names, so only plain identifiers are accepted.
fn table_name(collection: &str) -> Result<&str, RuntimeError> {
    let ok = !collection.is_empty()
        && collection.len() <= 63
        && collection
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_')
        && !collection.starts_with(|c: char| c.is_ascii_digit());
    if ok {
        Ok(collection)
    } else {
        err(format!(
            "`{collection}` is not a valid collection name (letters, digits and `_` only)"
        ))
    }
}

fn vector_literal(v: &[f32]) -> String {
    format!(
        "[{}]",
        v.iter()
            .map(|x| x.to_string())
            .collect::<Vec<_>>()
            .join(",")
    )
}

impl PgStore {
    pub(crate) fn connect(url: &str, dim: usize) -> Result<Self, RuntimeError> {
        let mut client = postgres::Client::connect(url, postgres::NoTls)
            .map_err(|e| RuntimeError::new(format!("cannot connect to PostgreSQL: {e}")))?;
        client
            .batch_execute("CREATE EXTENSION IF NOT EXISTS vector")
            .map_err(|e| {
                RuntimeError::new(format!("the pgvector extension is not available: {e}"))
            })?;
        Ok(Self {
            client: Mutex::new(client),
            ready: Mutex::new(HashSet::new()),
            dim,
        })
    }

    fn ensure(&self, client: &mut postgres::Client, table: &str) -> Result<(), RuntimeError> {
        let mut ready = self.ready.lock().unwrap_or_else(|e| e.into_inner());
        if ready.contains(table) {
            return Ok(());
        }
        client
            .batch_execute(&format!(
                "CREATE TABLE IF NOT EXISTS \"{table}\" (id text PRIMARY KEY, text text NOT NULL, embedding vector({}) NOT NULL)",
                self.dim
            ))
            .map_err(|e| RuntimeError::new(format!("cannot create table `{table}`: {e}")))?;
        ready.insert(table.to_string());
        Ok(())
    }

    pub(crate) fn insert(
        &self,
        collection: &str,
        id: &str,
        text: &str,
        vector: &[f32],
    ) -> Result<(), RuntimeError> {
        let table = table_name(collection)?;
        if vector.len() != self.dim {
            return err(format!(
                "vectors must have {} dimensions, got {}",
                self.dim,
                vector.len()
            ));
        }
        let mut client = self.client.lock().unwrap_or_else(|e| e.into_inner());
        self.ensure(&mut client, table)?;
        client
            .execute(
                &format!(
                    "INSERT INTO \"{table}\" (id, text, embedding) VALUES ($1, $2, $3::text::vector) \
                     ON CONFLICT (id) DO UPDATE SET text = EXCLUDED.text, embedding = EXCLUDED.embedding"
                ),
                &[&id, &text, &vector_literal(vector)],
            )
            .map(|_| ())
            .map_err(|e| RuntimeError::new(format!("insert failed: {e}")))
    }

    /// Nearest neighbours by cosine distance, as `(id, text, similarity)`.
    pub(crate) fn search(
        &self,
        collection: &str,
        vector: &[f32],
        limit: usize,
    ) -> Result<Vec<(String, String, f64)>, RuntimeError> {
        let table = table_name(collection)?;
        let mut client = self.client.lock().unwrap_or_else(|e| e.into_inner());
        self.ensure(&mut client, table)?;
        let rows = client
            .query(
                &format!(
                    "SELECT id, text, (1 - (embedding <=> $1::text::vector))::float8 AS score FROM \"{table}\" \
                     ORDER BY embedding <=> $1::text::vector, id LIMIT $2::bigint"
                ),
                &[&vector_literal(vector), &(limit as i64)],
            )
            .map_err(|e| RuntimeError::new(format!("search failed: {e}")))?;
        Ok(rows
            .iter()
            .map(|r| (r.get(0), r.get(1), r.get(2)))
            .collect())
    }
}

// ---- SQL on PostgreSQL --------------------------------------------------------------------

use postgres::types::{IsNull, ToSql, Type, to_sql_checked};

use crate::value::{PgClient, PgParam};

impl PgParam {
    fn kind(&self) -> &'static str {
        match self {
            PgParam::Null => "None",
            PgParam::Text(_) => "a Text",
            PgParam::Int(_) => "an Int",
            PgParam::Float(_) => "a Float",
            PgParam::Bool(_) => "a Bool",
        }
    }
}

type SqlError = Box<dyn std::error::Error + Sync + Send>;

impl ToSql for PgParam {
    /// Converts to the type the server expects for the place, so an `Int` can fill an `int4`.
    fn to_sql(
        &self,
        ty: &Type,
        out: &mut postgres::types::private::BytesMut,
    ) -> Result<IsNull, SqlError> {
        match (self, ty) {
            (PgParam::Null, _) => Ok(IsNull::Yes),
            (PgParam::Int(i), &Type::INT2) => i16::try_from(*i)?.to_sql(ty, out),
            (PgParam::Int(i), &Type::INT4) => i32::try_from(*i)?.to_sql(ty, out),
            (PgParam::Int(i), &Type::INT8) => i.to_sql(ty, out),
            (PgParam::Int(i), &Type::FLOAT4) => (*i as f32).to_sql(ty, out),
            (PgParam::Int(i), &Type::FLOAT8) => (*i as f64).to_sql(ty, out),
            (PgParam::Float(x), &Type::FLOAT4) => (*x as f32).to_sql(ty, out),
            (PgParam::Float(x), &Type::FLOAT8) => x.to_sql(ty, out),
            (PgParam::Bool(b), &Type::BOOL) => b.to_sql(ty, out),
            (PgParam::Text(s), &Type::TEXT | &Type::VARCHAR | &Type::BPCHAR | &Type::NAME) => {
                s.as_str().to_sql(ty, out)
            }
            (PgParam::Text(s), &Type::JSON | &Type::JSONB) => {
                serde_json::from_str::<Json>(s)
                    .map_err(|e| format!("not valid JSON: {e}"))?
                    .to_sql(ty, out)
            }
            (other, ty) => Err(format!(
                "cannot use {} where the server expects `{ty}`; cast it in the SQL, such as `$1::text`",
                other.kind()
            )
            .into()),
        }
    }

    fn accepts(_: &Type) -> bool {
        true
    }

    to_sql_checked!();
}

/// One column of a row as JSON; `json` and `jsonb` become the text of the document.
fn column_json(row: &postgres::Row, i: usize) -> Result<Json, String> {
    let column = &row.columns()[i];
    let ty = column.type_();
    let fail = |e: postgres::Error| format!("cannot read column `{}`: {e}", column.name());
    Ok(match *ty {
        Type::BOOL => row
            .try_get::<_, Option<bool>>(i)
            .map_err(fail)?
            .map_or(Json::Null, Json::from),
        Type::INT2 => row
            .try_get::<_, Option<i16>>(i)
            .map_err(fail)?
            .map_or(Json::Null, Json::from),
        Type::INT4 => row
            .try_get::<_, Option<i32>>(i)
            .map_err(fail)?
            .map_or(Json::Null, Json::from),
        Type::INT8 => row
            .try_get::<_, Option<i64>>(i)
            .map_err(fail)?
            .map_or(Json::Null, Json::from),
        Type::FLOAT4 => row
            .try_get::<_, Option<f32>>(i)
            .map_err(fail)?
            .map_or(Json::Null, |x| Json::from(f64::from(x))),
        Type::FLOAT8 => row
            .try_get::<_, Option<f64>>(i)
            .map_err(fail)?
            .map_or(Json::Null, Json::from),
        Type::TEXT | Type::VARCHAR | Type::BPCHAR | Type::NAME => row
            .try_get::<_, Option<String>>(i)
            .map_err(fail)?
            .map_or(Json::Null, Json::from),
        Type::JSON | Type::JSONB => row
            .try_get::<_, Option<Json>>(i)
            .map_err(fail)?
            .map_or(Json::Null, |doc| Json::from(doc.to_string())),
        _ => {
            return Err(format!(
                "column `{}` has type `{ty}`, which is not supported; write `{}::text` in the SQL",
                column.name(),
                column.name()
            ));
        }
    })
}

/// What the server said, or what went wrong on the way to it.
fn pg_message(e: &postgres::Error) -> String {
    match e.as_db_error() {
        Some(db) => match db.detail() {
            Some(detail) => format!("{}: {detail}", db.message()),
            None => db.message().to_string(),
        },
        None => match std::error::Error::source(e) {
            Some(cause) => format!("{e}: {cause}"),
            None => e.to_string(),
        },
    }
}

impl PgClient {
    /// Runs `f` on the connection, opening it if there is none. A connection that was lost before
    /// the statement reached the server is opened again once.
    fn with_client<T>(
        &self,
        sql: &str,
        f: impl FnOnce(&mut postgres::Client, &postgres::Statement) -> Result<T, postgres::Error>,
    ) -> Result<T, String> {
        let mut conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let mut attempts = 0;
        loop {
            if conn.is_none() {
                let client = postgres::Client::connect(&self.url, postgres::NoTls)
                    .map_err(|e| format!("cannot connect to PostgreSQL: {e}"))?;
                *conn = Some(client);
            }
            let client = conn.as_mut().expect("just opened");
            match client.prepare(sql) {
                Ok(stmt) => {
                    let out = f(client, &stmt);
                    if out.as_ref().is_err_and(postgres::Error::is_closed) {
                        *conn = None;
                    }
                    return out.map_err(|e| pg_message(&e));
                }
                Err(e) if e.is_closed() && attempts == 0 => {
                    *conn = None;
                    attempts += 1;
                }
                Err(e) => {
                    if e.is_closed() {
                        *conn = None;
                    }
                    return Err(pg_message(&e));
                }
            }
        }
    }

    /// Runs a statement; the number of rows it changed.
    pub(crate) fn execute(&self, sql: &str, params: &[PgParam]) -> Result<i64, String> {
        let refs: Vec<&(dyn ToSql + Sync)> = params.iter().map(|p| p as _).collect();
        self.with_client(sql, |client, stmt| client.execute(stmt, &refs))
            .map(|n| n as i64)
    }

    /// Runs a query; every row as a JSON object of its columns.
    pub(crate) fn query(&self, sql: &str, params: &[PgParam]) -> Result<Vec<Json>, String> {
        let refs: Vec<&(dyn ToSql + Sync)> = params.iter().map(|p| p as _).collect();
        let rows = self.with_client(sql, |client, stmt| client.query(stmt, &refs))?;
        rows.iter()
            .map(|row| {
                let mut object = serde_json::Map::new();
                for (i, column) in row.columns().iter().enumerate() {
                    object.insert(column.name().to_string(), column_json(row, i)?);
                }
                Ok(Json::Object(object))
            })
            .collect()
    }
}
