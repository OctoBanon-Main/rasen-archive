use std::{
    env,
    fs::{self, File},
    io::{BufReader, BufWriter, Write},
    path::{Path, PathBuf},
};

use rasen_archive::{Archive, InputFile, PackOptions, pack_with_options};

const XOR_KEY: &[u8] = b"example-key";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    match args.as_slice() {
        [_, cmd, input_dir, output] if cmd == "pack" => {
            pack_dir(Path::new(input_dir), Path::new(output), PackOptions::default())?
        }
        [_, cmd, input_dir, output, chunk_kib] if cmd == "pack" => {
            let chunk_kib: usize = chunk_kib.parse()?;
            pack_dir(
                Path::new(input_dir),
                Path::new(output),
                PackOptions {
                    chunk_size: chunk_kib.checked_mul(1024).ok_or_else(chunk_size_overflow)?,
                    ..PackOptions::default()
                },
            )?
        }
        [_, cmd, input_dir, output, chunk_kib, alignment] if cmd == "pack" => {
            let chunk_kib: usize = chunk_kib.parse()?;
            let alignment: u32 = alignment.parse()?;
            pack_dir(
                Path::new(input_dir),
                Path::new(output),
                PackOptions {
                    chunk_size: chunk_kib.checked_mul(1024).ok_or_else(chunk_size_overflow)?,
                    alignment,
                },
            )?
        }
        [_, cmd, archive] if cmd == "list" => list_archive(Path::new(archive))?,
        [_, cmd, archive, path, output] if cmd == "extract" => {
            extract_one(Path::new(archive), path, Path::new(output))?
        }
        _ => print_usage(),
    }
    Ok(())
}

fn chunk_size_overflow() -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidInput, "chunk size overflow")
}

fn print_usage() {
    eprintln!("usage:");
    eprintln!("  rasen-pack pack <input-dir> <archive.rpak> [chunk-kib] [alignment]");
    eprintln!("  rasen-pack list <archive.rpak>");
    eprintln!("  rasen-pack extract <archive.rpak> <virtual-path> <output-file>");
}

fn pack_dir(
    root: &Path,
    output: &Path,
    options: PackOptions,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut disk_files = Vec::new();
    collect_files(root, &mut disk_files)?;
    disk_files.sort();

    let mut files = Vec::with_capacity(disk_files.len());
    for disk_path in disk_files {
        let rel = disk_path.strip_prefix(root)?;
        let virtual_path = rel.to_string_lossy().replace('\\', "/");
        files.push(InputFile {
            path: virtual_path,
            data: fs::read(&disk_path)?,
        });
    }

    let file = File::create(output)?;
    let mut writer = BufWriter::new(file);
    pack_with_options(&mut writer, &files, XOR_KEY, options)?;
    writer.flush()?;

    println!(
        "packed {} files -> {} (chunk={} KiB, align={})",
        files.len(),
        output.display(),
        options.chunk_size / 1024,
        options.alignment
    );
    Ok(())
}

fn list_archive(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let file = File::open(path)?;
    let archive = Archive::open(BufReader::new(file), XOR_KEY)?;
    println!(
        "chunk={} bytes, alignment={} bytes",
        archive.chunk_size(),
        archive.alignment()
    );
    for e in archive.entries() {
        let ratio = if e.original_size == 0 {
            1.0
        } else {
            e.stored_size as f64 / e.original_size as f64
        };
        println!(
            "{:016x}  chunks={:<4} stored={:<10} original={:<10} ratio={:.3}  {}",
            e.path_hash,
            e.chunk_count,
            e.stored_size,
            e.original_size,
            ratio,
            e.path
        );
    }
    Ok(())
}

fn extract_one(
    archive_path: &Path,
    virtual_path: &str,
    output: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let file = File::open(archive_path)?;
    let mut archive = Archive::open(BufReader::new(file), XOR_KEY)?;
    let data = archive.read(virtual_path)?;
    fs::write(output, data)?;
    Ok(())
}

fn collect_files(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let ty = entry.file_type()?;
        if ty.is_dir() {
            collect_files(&path, out)?;
        } else if ty.is_file() {
            out.push(path);
        }
    }
    Ok(())
}