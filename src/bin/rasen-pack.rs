use std::{
    env,
    ffi::OsString,
    fs::{self, File},
    io::{BufWriter, Write},
    path::{self, Path, PathBuf},
    process,
    time::{SystemTime, UNIX_EPOCH},
};

use rasen_archive::{Archive, ArchiveLimits, PackMode, PackOptions, Packer, normalize_path};

const DEFAULT_XOR_KEY: &str = "example-key";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args: Vec<String> = env::args().skip(1).collect();
    let key = take_key(&mut args)?;
    match args.as_slice() {
        [flag] if flag == "--help" || flag == "-h" => print_usage(),
        [flag] if flag == "--version" || flag == "-V" => {
            println!("rasen-pack {}", env!("CARGO_PKG_VERSION"));
        }
        [cmd, input_dir, output, rest @ ..] if cmd == "pack" => {
            let options = parse_pack_options(rest)?;
            pack_dir(
                Path::new(input_dir),
                Path::new(output),
                options,
                key.as_bytes(),
            )?
        }
        [cmd, archive] if cmd == "list" => list_archive(Path::new(archive), key.as_bytes())?,
        [cmd, archive] if cmd == "verify" => verify_archive(Path::new(archive), key.as_bytes())?,
        [cmd, archive] if cmd == "info" => info_archive(Path::new(archive), key.as_bytes())?,
        [cmd, archive, virtual_path, output] if cmd == "extract" => extract_one(
            Path::new(archive),
            virtual_path,
            Path::new(output),
            key.as_bytes(),
        )?,
        _ => return Err(invalid_arg("invalid command or arguments; use --help").into()),
    }
    Ok(())
}

fn take_key(args: &mut Vec<String>) -> Result<String, Box<dyn std::error::Error>> {
    let mut key = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--key" => {
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| invalid_arg("--key requires a value"))?
                    .clone();
                args.drain(index..=index + 1);
                key = Some(value);
            }
            value if value.starts_with("--key=") => {
                key = Some(value["--key=".len()..].to_owned());
                args.remove(index);
            }
            _ => index += 1,
        }
    }
    Ok(key
        .or_else(|| env::var("RPAK_XOR_KEY").ok())
        .unwrap_or_else(|| DEFAULT_XOR_KEY.to_owned()))
}

fn parse_pack_options(args: &[String]) -> Result<PackOptions, Box<dyn std::error::Error>> {
    let mut options = PackOptions::default();
    let mut positional = Vec::new();
    let mut i = 0;

    while i < args.len() {
        match args[i].as_str() {
            "--debug" => options.mode = PackMode::Debug,
            "--production" | "--prod" => options.mode = PackMode::Production,
            "--mode" => {
                i += 1;
                let value = args
                    .get(i)
                    .ok_or_else(|| invalid_arg("--mode requires a value"))?;
                options.mode = parse_mode(value)?;
            }
            value if value.starts_with("--mode=") => {
                options.mode = parse_mode(&value["--mode=".len()..])?;
            }
            value if value.starts_with('-') => {
                return Err(invalid_arg(format!("unknown pack option: {value}")).into());
            }
            value => positional.push(value),
        }
        i += 1;
    }

    match positional.as_slice() {
        [] => {}
        [chunk_kib] => {
            options.chunk_size = chunk_kib
                .parse::<usize>()?
                .checked_mul(1024)
                .ok_or_else(chunk_size_overflow)?;
        }
        [chunk_kib, alignment] => {
            options.chunk_size = chunk_kib
                .parse::<usize>()?
                .checked_mul(1024)
                .ok_or_else(chunk_size_overflow)?;
            options.alignment = alignment.parse()?;
        }
        _ => return Err(invalid_arg("too many positional pack arguments").into()),
    }

    Ok(options)
}

fn parse_mode(value: &str) -> Result<PackMode, Box<dyn std::error::Error>> {
    match value {
        "debug" => Ok(PackMode::Debug),
        "production" | "prod" => Ok(PackMode::Production),
        _ => Err(invalid_arg(format!(
            "invalid pack mode '{value}', expected debug or production"
        ))
        .into()),
    }
}

fn invalid_arg(message: impl Into<String>) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidInput, message.into())
}

fn chunk_size_overflow() -> std::io::Error {
    invalid_arg("chunk size overflow")
}

fn print_usage() {
    println!("usage:");
    println!(
        "  rasen-pack pack <input-dir> <archive.rpak> [chunk-kib] [alignment] [--debug|--production] [--key <key>]"
    );
    println!("  rasen-pack list <archive.rpak> [--key <key>]");
    println!("  rasen-pack verify <archive.rpak> [--key <key>]");
    println!("  rasen-pack info <archive.rpak> [--key <key>]");
    println!("  rasen-pack extract <archive.rpak> <virtual-path> <output-file> [--key <key>]");
}

fn pack_dir(
    root: &Path,
    output: &Path,
    options: PackOptions,
    xor_key: &[u8],
) -> Result<(), Box<dyn std::error::Error>> {
    let root = fs::canonicalize(root)?;
    let output = normalized_output_path(output)?;
    let existing_output = match fs::canonicalize(&output) {
        Ok(path) => Some(path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.into()),
    };
    let mut files = collect_files(&root, &output, existing_output.as_deref())?;
    files.sort_unstable_by(|left, right| left.0.as_bytes().cmp(right.0.as_bytes()));

    let (temp_path, temp_file) = create_temp(&output)?;
    let result = (|| {
        let mut writer = BufWriter::new(temp_file);
        let summary = {
            let mut packer = Packer::new(&mut writer, xor_key, options)?;
            for (virtual_path, disk_path) in &files {
                let mut source = File::open(disk_path)?;
                packer.add_reader(virtual_path, &mut source)?;
            }
            packer.finish()?
        };
        writer.flush()?;
        writer.get_ref().sync_all()?;
        drop(writer);
        fs::rename(&temp_path, &output)?;
        Ok::<_, Box<dyn std::error::Error>>(summary)
    })();

    let summary = match result {
        Ok(summary) => summary,
        Err(error) => {
            match fs::remove_file(&temp_path) {
                Err(cleanup) if cleanup.kind() != std::io::ErrorKind::NotFound => eprintln!(
                    "warning: could not remove temporary file {}: {cleanup}",
                    temp_path.display()
                ),
                _ => {}
            }
            return Err(error);
        }
    };

    println!(
        "packed {} files, {} chunks, {} bytes -> {} (mode={:?}, chunk={} KiB, align={})",
        summary.entry_count,
        summary.chunk_count,
        summary.archive_len,
        output.display(),
        options.mode,
        options.chunk_size / 1024,
        options.alignment
    );
    Ok(())
}

fn list_archive(path: &Path, xor_key: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    let archive =
        Archive::open_with_limits(File::open(path)?, xor_key, ArchiveLimits::tooling_default())?;
    println!(
        "mode={}, chunk={} bytes, alignment={} bytes",
        match archive.paths_stripped() {
            true => "production",
            false => "debug",
        },
        archive.chunk_size(),
        archive.alignment()
    );
    for entry in archive.entries() {
        let ratio = match entry.original_size {
            0 => 1.0,
            size => entry.stored_size as f64 / size as f64,
        };
        let path = match archive.paths_stripped() {
            true => "<path stripped>",
            false => entry.path.as_str(),
        };
        println!(
            "{:016x}  chunks={:<4} stored={:<10} original={:<10} ratio={:.3}  {}",
            entry.path_hash, entry.chunk_count, entry.stored_size, entry.original_size, ratio, path
        );
    }
    Ok(())
}

fn verify_archive(path: &Path, xor_key: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    let archive =
        Archive::open_with_limits(File::open(path)?, xor_key, ArchiveLimits::tooling_default())?;
    archive.verify()?;
    let mut chunk_count = 0u64;
    for entry in archive.entries() {
        chunk_count = chunk_count
            .checked_add(u64::from(entry.chunk_count))
            .ok_or_else(|| invalid_arg("chunk count overflow"))?;
    }
    println!(
        "verified {} entries, {chunk_count} chunks",
        archive.entries().len()
    );
    Ok(())
}

fn info_archive(path: &Path, xor_key: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    let file = File::open(path)?;
    let archive_size = file.metadata()?.len();
    let archive = Archive::open_with_limits(file, xor_key, ArchiveLimits::tooling_default())?;
    let mut chunk_count = 0u64;
    let mut original_bytes = 0u64;
    let mut stored_bytes = 0u64;
    for entry in archive.entries() {
        chunk_count = chunk_count
            .checked_add(u64::from(entry.chunk_count))
            .ok_or_else(|| invalid_arg("chunk count overflow"))?;
        original_bytes = original_bytes
            .checked_add(entry.original_size)
            .ok_or_else(|| invalid_arg("original byte count overflow"))?;
        stored_bytes = stored_bytes
            .checked_add(entry.stored_size)
            .ok_or_else(|| invalid_arg("stored byte count overflow"))?;
    }
    let ratio = match original_bytes {
        0 => 1.0,
        size => stored_bytes as f64 / size as f64,
    };

    println!("format_version={}", rasen_archive::VERSION);
    println!(
        "mode={}",
        if archive.paths_stripped() {
            "production"
        } else {
            "debug"
        }
    );
    println!("archive_bytes={archive_size}");
    println!("entry_count={}", archive.entries().len());
    println!("chunk_count={chunk_count}");
    println!("chunk_size={}", archive.chunk_size());
    println!("alignment={}", archive.alignment());
    println!("original_bytes={original_bytes}");
    println!("stored_bytes={stored_bytes}");
    println!("compression_ratio={ratio:.6}");
    println!("paths_stripped={}", archive.paths_stripped());
    Ok(())
}

fn extract_one(
    archive_path: &Path,
    virtual_path: &str,
    output: &Path,
    xor_key: &[u8],
) -> Result<(), Box<dyn std::error::Error>> {
    let archive = Archive::open_with_limits(
        File::open(archive_path)?,
        xor_key,
        ArchiveLimits::tooling_default(),
    )?;
    fs::write(output, archive.read(virtual_path)?)?;
    Ok(())
}

fn normalized_output_path(output: &Path) -> std::io::Result<PathBuf> {
    let absolute = path::absolute(output)?;
    let name = absolute
        .file_name()
        .ok_or_else(|| invalid_arg("output has no file name"))?;
    let parent = fs::canonicalize(
        absolute
            .parent()
            .ok_or_else(|| invalid_arg("output has no parent directory"))?,
    )?;
    Ok(parent.join(name))
}

fn collect_files(
    root: &Path,
    output: &Path,
    existing_output: Option<&Path>,
) -> Result<Vec<(String, PathBuf)>, Box<dyn std::error::Error>> {
    let mut files = Vec::new();
    let mut pending = vec![root.to_owned()];
    while let Some(dir) = pending.pop() {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let file_type = entry.file_type()?;
            let disk_path = entry.path();
            let excluded = disk_path == output || existing_output == Some(disk_path.as_path());
            match (file_type.is_dir(), file_type.is_file(), excluded) {
                (true, _, _) => pending.push(disk_path),
                (false, true, false) => {
                    let relative = disk_path.strip_prefix(root)?;
                    let path = relative.to_str().ok_or_else(|| {
                        invalid_arg(format!(
                            "input path is not valid UTF-8: {}",
                            relative.display()
                        ))
                    })?;
                    files.push((normalize_path(path)?, disk_path));
                }
                _ => {}
            }
        }
    }
    Ok(files)
}

fn create_temp(output: &Path) -> std::io::Result<(PathBuf, File)> {
    let parent = output
        .parent()
        .ok_or_else(|| invalid_arg("output has no parent directory"))?;
    let file_name = output
        .file_name()
        .ok_or_else(|| invalid_arg("output has no file name"))?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    for attempt in 0..128 {
        let mut name = OsString::from(file_name);
        name.push(format!(".tmp.{}.{stamp:x}.{attempt}", process::id()));
        let path = parent.join(name);
        match File::create_new(&path) {
            Ok(file) => return Ok((path, file)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "could not allocate temporary output name",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let path = env::temp_dir().join(format!(
            "rasen-pack-{name}-{}-{}",
            process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&path).unwrap();
        path
    }

    #[test]
    fn output_inside_input_is_excluded() {
        let root = temp_dir("self-exclusion");
        fs::write(root.join("asset.txt"), b"asset").unwrap();
        let output = root.join("content.rpak");
        fs::write(&output, b"old archive").unwrap();

        pack_dir(&root, &output, PackOptions::default(), b"test-key").unwrap();
        let archive = Archive::open(File::open(&output).unwrap(), b"test-key").unwrap();
        assert_eq!(archive.entries().len(), 1);
        assert_eq!(archive.read("asset.txt").unwrap(), b"asset");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn failed_pack_preserves_existing_output() {
        let root = temp_dir("failure-safe");
        fs::write(root.join("asset.txt"), b"asset").unwrap();
        let output = root.join("content.rpak");
        fs::write(&output, b"old archive").unwrap();
        let invalid = PackOptions {
            chunk_size: 0,
            ..PackOptions::default()
        };

        assert!(pack_dir(&root, &output, invalid, b"test-key").is_err());
        assert_eq!(fs::read(&output).unwrap(), b"old archive");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn packing_is_deterministic_and_custom_key_roundtrips() {
        let first = temp_dir("deterministic-first");
        let second = temp_dir("deterministic-second");
        fs::write(first.join("b.bin"), b"b").unwrap();
        fs::write(first.join("a.bin"), b"a").unwrap();
        fs::write(second.join("a.bin"), b"a").unwrap();
        fs::write(second.join("b.bin"), b"b").unwrap();
        let first_output = first.join("content.rpak");
        let second_output = second.join("content.rpak");

        pack_dir(&first, &first_output, PackOptions::default(), b"custom-key").unwrap();
        pack_dir(
            &second,
            &second_output,
            PackOptions::default(),
            b"custom-key",
        )
        .unwrap();
        assert_eq!(
            fs::read(&first_output).unwrap(),
            fs::read(&second_output).unwrap()
        );
        list_archive(&first_output, b"custom-key").unwrap();
        verify_archive(&first_output, b"custom-key").unwrap();
        info_archive(&first_output, b"custom-key").unwrap();
        let extracted = first.join("extracted.bin");
        extract_one(&first_output, "a.bin", &extracted, b"custom-key").unwrap();
        assert_eq!(fs::read(extracted).unwrap(), b"a");

        fs::remove_dir_all(first).unwrap();
        fs::remove_dir_all(second).unwrap();
    }

    #[test]
    fn verify_reports_payload_corruption() {
        let root = temp_dir("verify-corruption");
        fs::write(root.join("asset.bin"), vec![7; 4096]).unwrap();
        let output = root.join("content.rpak");
        pack_dir(&root, &output, PackOptions::default(), b"test-key").unwrap();

        let mut bytes = fs::read(&output).unwrap();
        let first_chunk = usize::try_from(rasen_archive::HEADER_SIZE.next_multiple_of(16)).unwrap();
        bytes[first_chunk] ^= 1;
        fs::write(&output, bytes).unwrap();

        assert!(verify_archive(&output, b"test-key").is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn verify_supports_production_archives() {
        let root = temp_dir("verify-production");
        fs::write(root.join("asset.bin"), vec![7; 4096]).unwrap();
        let output = root.join("content.rpak");
        let options = PackOptions {
            mode: PackMode::Production,
            ..PackOptions::default()
        };
        pack_dir(&root, &output, options, b"test-key").unwrap();

        verify_archive(&output, b"test-key").unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_input_path_is_rejected() {
        use std::{ffi::OsString, os::unix::ffi::OsStringExt};

        let root = temp_dir("non-utf8");
        fs::write(root.join(OsString::from_vec(vec![0xff])), b"asset").unwrap();
        let output = root.join("content.rpak");
        assert!(pack_dir(&root, &output, PackOptions::default(), b"key").is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
