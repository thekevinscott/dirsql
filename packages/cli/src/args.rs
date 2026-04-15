//! CLI argument parsing.

use clap::Parser;
use std::path::PathBuf;

/// `dirsql` serves an HTTP query API over a directory.
#[derive(Debug, Parser, Clone)]
#[command(
    name = "dirsql",
    version,
    about = "Run an HTTP query server over a directory"
)]
pub struct Args {
    /// Directory to serve. Defaults to the current working directory.
    #[arg(value_name = "DIR")]
    pub dir: Option<PathBuf>,

    /// Path to the config file. Defaults to <DIR>/.dirsql.toml.
    #[arg(long, value_name = "PATH")]
    pub config: Option<PathBuf>,

    /// TCP port to listen on.
    #[arg(long, default_value_t = 4321)]
    pub port: u16,

    /// Host address to bind.
    #[arg(long, default_value = "127.0.0.1")]
    pub host: String,
}

#[derive(Debug, Clone)]
pub struct Resolved {
    pub dir: PathBuf,
    pub config: PathBuf,
    pub port: u16,
    pub host: String,
}

impl Args {
    /// Resolve defaults against a given cwd. Extracted for testing.
    pub fn resolve_with_cwd(&self, cwd: &std::path::Path) -> Resolved {
        let dir = match &self.dir {
            Some(d) if d.is_absolute() => d.clone(),
            Some(d) => cwd.join(d),
            None => cwd.to_path_buf(),
        };
        let config = match &self.config {
            Some(c) if c.is_absolute() => c.clone(),
            Some(c) => cwd.join(c),
            None => dir.join(".dirsql.toml"),
        };
        Resolved {
            dir,
            config,
            port: self.port,
            host: self.host.clone(),
        }
    }

    pub fn resolve(&self) -> std::io::Result<Resolved> {
        let cwd = std::env::current_dir()?;
        Ok(self.resolve_with_cwd(&cwd))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use std::path::PathBuf;

    #[test]
    fn defaults() {
        let a = Args::parse_from(["dirsql"]);
        assert_eq!(a.port, 4321);
        assert_eq!(a.host, "127.0.0.1");
        assert!(a.dir.is_none());
        assert!(a.config.is_none());
    }

    #[test]
    fn resolve_dir_and_config_defaults() {
        let a = Args::parse_from(["dirsql"]);
        let cwd = PathBuf::from("/abs/cwd");
        let r = a.resolve_with_cwd(&cwd);
        assert_eq!(r.dir, PathBuf::from("/abs/cwd"));
        assert_eq!(r.config, PathBuf::from("/abs/cwd/.dirsql.toml"));
    }

    #[test]
    fn relative_dir_made_absolute_to_cwd() {
        let a = Args::parse_from(["dirsql", "subdir"]);
        let r = a.resolve_with_cwd(std::path::Path::new("/abs"));
        assert_eq!(r.dir, PathBuf::from("/abs/subdir"));
    }

    #[test]
    fn explicit_config_honored() {
        let a = Args::parse_from(["dirsql", "--config", "/etc/my.toml"]);
        let r = a.resolve_with_cwd(std::path::Path::new("/abs"));
        assert_eq!(r.config, PathBuf::from("/etc/my.toml"));
    }

    #[test]
    fn port_flag_parsed() {
        let a = Args::parse_from(["dirsql", "--port", "9999"]);
        assert_eq!(a.port, 9999);
    }

    #[test]
    fn host_flag_parsed() {
        let a = Args::parse_from(["dirsql", "--host", "0.0.0.0"]);
        assert_eq!(a.host, "0.0.0.0");
    }
}
