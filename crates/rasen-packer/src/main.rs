mod commands;

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};
use rasen_archive::{PackMode, PackOptions, Protection};

use commands::{extract_one, info_archive, list_archive, pack_dir, verify_archive};

const DEFAULT_KEY: &str = "example-key";

#[derive(Parser)]
#[command(version, about)]
struct Cli {
    #[arg(long, global = true, env = "RPAK_KEY", default_value = DEFAULT_KEY)]
    key: String,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Pack a directory into an archive.
    Pack(PackArgs),
    /// List archive entries.
    List { archive: PathBuf },
    /// Verify every archive chunk.
    Verify { archive: PathBuf },
    /// Show archive metadata.
    Info { archive: PathBuf },
    /// Extract one archive entry.
    Extract {
        archive: PathBuf,
        virtual_path: String,
        output: PathBuf,
    },
}

#[derive(Args)]
struct PackArgs {
    input_dir: PathBuf,
    archive: PathBuf,
    #[arg(long, value_enum, default_value = "debug")]
    mode: Mode,
    #[arg(long, value_enum, default_value = "xor")]
    protection: ProtectionArg,
    #[arg(long)]
    chunk_kib: Option<usize>,
    #[arg(long)]
    alignment: Option<u32>,
}

impl PackArgs {
    fn options(&self) -> Result<PackOptions, std::io::Error> {
        let mut options = PackOptions {
            mode: self.mode.into(),
            protection: self.protection.into(),
            ..PackOptions::default()
        };
        if let Some(chunk_kib) = self.chunk_kib {
            options.chunk_size = chunk_kib.checked_mul(1024).ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::InvalidInput, "chunk size overflow")
            })?;
        }
        if let Some(alignment) = self.alignment {
            options.alignment = alignment;
        }
        Ok(options)
    }
}

#[derive(Copy, Clone, ValueEnum)]
enum Mode {
    Debug,
    Production,
}

impl From<Mode> for PackMode {
    fn from(value: Mode) -> Self {
        match value {
            Mode::Debug => Self::Debug,
            Mode::Production => Self::Production,
        }
    }
}

#[derive(Copy, Clone, ValueEnum)]
enum ProtectionArg {
    Xor,
    Aead,
}

impl From<ProtectionArg> for Protection {
    fn from(value: ProtectionArg) -> Self {
        match value {
            ProtectionArg::Xor => Self::Xor,
            ProtectionArg::Aead => Self::Aead,
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let Cli { key, command } = Cli::parse();
    match command {
        Command::Pack(args) => pack_dir(
            &args.input_dir,
            &args.archive,
            args.options()?,
            key.as_bytes(),
        )?,
        Command::List { archive } => list_archive(&archive, key.as_bytes())?,
        Command::Verify { archive } => verify_archive(&archive, key.as_bytes())?,
        Command::Info { archive } => info_archive(&archive, key.as_bytes())?,
        Command::Extract {
            archive,
            virtual_path,
            output,
        } => extract_one(&archive, &virtual_path, &output, key.as_bytes())?,
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clap_parses_pack_options() {
        let cli = Cli::try_parse_from([
            "rasen-pack",
            "pack",
            "input",
            "archive.rpak",
            "--mode=production",
            "--protection=aead",
            "--chunk-kib=256",
            "--alignment=4096",
        ])
        .unwrap();
        let Command::Pack(args) = cli.command else {
            panic!("expected pack command");
        };
        let options = args.options().unwrap();
        assert_eq!(options.mode, PackMode::Production);
        assert_eq!(options.protection, Protection::Aead);
        assert_eq!(options.chunk_size, 256 * 1024);
        assert_eq!(options.alignment, 4096);
        assert!(
            Cli::try_parse_from(["rasen-packer", "pack", "input", "archive.rpak", "--debug"])
                .is_err()
        );
    }
}
