//! What a program may do outside itself: read and write files, read the environment, reach the
//! network. By default everything is allowed; `--allow` narrows it.

use std::fmt;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Capability {
    FsRead,
    FsWrite,
    Env,
    Net,
}

impl Capability {
    const ALL: [Capability; 4] = [
        Capability::FsRead,
        Capability::FsWrite,
        Capability::Env,
        Capability::Net,
    ];

    fn bit(self) -> u8 {
        match self {
            Capability::FsRead => 1,
            Capability::FsWrite => 2,
            Capability::Env => 4,
            Capability::Net => 8,
        }
    }

    /// The word used in `--allow` and in error messages.
    pub fn name(self) -> &'static str {
        match self {
            Capability::FsRead => "fs.read",
            Capability::FsWrite => "fs.write",
            Capability::Env => "env",
            Capability::Net => "net",
        }
    }

    /// The capability a standard library function needs, if it needs one.
    pub fn of_function(path: &str) -> Option<Capability> {
        Some(match path {
            "std::fs::read" | "std::fs::exists" => Capability::FsRead,
            "std::table::read_csv"
            | "std::table::read_parquet"
            | "std::table::read_ipc"
            | "std::table::read_csv_batches"
            | "std::table::read_parquet_batches"
            | "std::table::read_ipc_batches" => Capability::FsRead,
            "std::fs::write"
            | "std::table::write_csv"
            | "std::table::write_parquet"
            | "std::table::write_ipc" => Capability::FsWrite,
            "std::env::get" => Capability::Env,
            "std::table::read_flight"
            | "std::table::write_flight"
            | "std::ai::embeddings::OllamaEmbedder::new"
            | "std::ai::llm::OpenAIClient::new"
            | "std::db::pgvector::PgVectorClient::connect"
            | "std::db::postgres::PostgresClient::execute"
            | "std::db::postgres::PostgresClient::query" => Capability::Net,
            _ => return None,
        })
    }
}

/// A set of capabilities.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Caps(u8);

impl Caps {
    pub const ALL: Caps = Caps(15);
    pub const NONE: Caps = Caps(0);

    pub fn allows(self, c: Capability) -> bool {
        self.0 & c.bit() != 0
    }

    pub(crate) fn bits(self) -> u8 {
        self.0
    }

    pub(crate) fn from_bits(bits: u8) -> Caps {
        Caps(bits & 15)
    }

    /// Parses the value of `--allow`: words separated by commas, from `fs.read`, `fs.write`,
    /// `fs` (both), `env`, `net`, `all` and `none`.
    pub fn parse(list: &str) -> Result<Caps, String> {
        let mut bits = 0;
        for word in list.split(',').map(str::trim).filter(|w| !w.is_empty()) {
            bits |= match word {
                "all" => Caps::ALL.0,
                "none" => 0,
                "fs" => Capability::FsRead.bit() | Capability::FsWrite.bit(),
                other => match Capability::ALL.iter().find(|c| c.name() == other) {
                    Some(c) => c.bit(),
                    None => {
                        return Err(format!(
                            "unknown capability `{other}`; write one of: fs.read, fs.write, fs, env, net, all, none"
                        ));
                    }
                },
            };
        }
        Ok(Caps(bits))
    }
}

impl fmt::Debug for Caps {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let names: Vec<_> = Capability::ALL
            .iter()
            .filter(|c| self.allows(**c))
            .map(|c| c.name())
            .collect();
        write!(f, "Caps({})", names.join(","))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_lists_and_the_fs_shorthand() {
        let caps = Caps::parse("fs, net").unwrap();
        assert!(caps.allows(Capability::FsRead) && caps.allows(Capability::FsWrite));
        assert!(caps.allows(Capability::Net) && !caps.allows(Capability::Env));
        assert_eq!(Caps::parse("none").unwrap(), Caps::NONE);
        assert_eq!(Caps::parse("all").unwrap(), Caps::ALL);
        assert_eq!(Caps::parse("fs.read").unwrap().bits(), 1);
    }

    #[test]
    fn an_unknown_word_lists_the_ones_that_exist() {
        let e = Caps::parse("fs.read,disk").unwrap_err();
        assert!(
            e.contains("unknown capability `disk`") && e.contains("fs.write"),
            "{e}"
        );
    }

    #[test]
    fn functions_map_to_what_they_touch() {
        assert_eq!(
            Capability::of_function("std::fs::write"),
            Some(Capability::FsWrite)
        );
        assert_eq!(
            Capability::of_function("std::table::read_csv"),
            Some(Capability::FsRead)
        );
        assert_eq!(Capability::of_function("std::math::abs"), None);
    }
}
