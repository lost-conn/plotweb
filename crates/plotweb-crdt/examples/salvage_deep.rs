//! Deep salvage: parse yrs update binaries block-by-block (using only *public*
//! yrs APIs: `Decoder`/`DecoderV1`/`ItemContent::decode`) and print every
//! `ItemContent::String` payload in encounter order, across every blob in a
//! directory (sorted by filename). This recovers typed text even when the
//! generation's base snapshot is missing and `apply_update` can't integrate
//! anything (see replay.rs's `--salvage`, which only catches runs of >=3
//! printable bytes and therefore misses single/double-character keystroke
//! deltas). We can't reach yrs's private `Update::decode_block` directly
//! (its fields/helpers are `pub(crate)`), so this reimplements just enough
//! of that byte layout using the public `Decoder` trait + `ItemContent::decode`.
//!
//!     cargo run -p plotweb-crdt --example salvage_deep -- <dir>

use std::path::PathBuf;
use yrs::block::{
    BLOCK_GC_REF_NUMBER, BLOCK_SKIP_REF_NUMBER, HAS_ORIGIN, HAS_PARENT_SUB, HAS_RIGHT_ORIGIN,
    ItemContent,
};
use yrs::encoding::read::Error;
use yrs::updates::decoder::{Decoder, DecoderV1};

fn blobs_in(dir: &str) -> Vec<(PathBuf, Vec<u8>)> {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .expect("could not read the blob directory")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_file() && !p.file_name().unwrap().to_string_lossy().starts_with('_'))
        .collect();
    entries.sort_by_key(|p| {
        let name = p.file_name().unwrap().to_string_lossy().to_string();
        (!name.contains("snapshot"), name)
    });
    entries
        .into_iter()
        .map(|p| {
            let bytes = std::fs::read(&p).expect("could not read a blob");
            (p, bytes)
        })
        .collect()
}

/// Decode one update's blocks, pushing each String item's text into `out` in
/// order. Mirrors yrs's private `Update::decode_block` layout, using only
/// public API surface.
fn extract_strings<D: Decoder>(decoder: &mut D, out: &mut Vec<String>) -> Result<(), Error> {
    let clients_len: u32 = decoder.read_var()?;
    for _ in 0..clients_len {
        let blocks_len: u32 = decoder.read_var()?;
        let _client = decoder.read_client()?;
        let _clock: u32 = decoder.read_var()?;
        for _ in 0..blocks_len {
            let info = decoder.read_info()?;
            match info {
                BLOCK_SKIP_REF_NUMBER => {
                    let _len: u32 = decoder.read_var()?;
                }
                BLOCK_GC_REF_NUMBER => {
                    let _len: u32 = decoder.read_len()?;
                }
                info => {
                    let cant_copy_parent_info = info & (HAS_ORIGIN | HAS_RIGHT_ORIGIN) == 0;
                    if info & HAS_ORIGIN != 0 {
                        let _ = decoder.read_left_id()?;
                    }
                    if info & HAS_RIGHT_ORIGIN != 0 {
                        let _ = decoder.read_right_id()?;
                    }
                    if cant_copy_parent_info {
                        if decoder.read_parent_info()? {
                            let _ = decoder.read_string()?;
                        } else {
                            let _ = decoder.read_left_id()?;
                        }
                    }
                    if cant_copy_parent_info && (info & HAS_PARENT_SUB != 0) {
                        let _ = decoder.read_string()?;
                    }
                    let content = ItemContent::decode(decoder, info)?;
                    if let ItemContent::String(s) = content {
                        out.push(s.to_string());
                    } else if let ItemContent::JSON(items) = content {
                        for i in items {
                            out.push(i);
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

fn main() {
    let mut args = std::env::args().skip(1);
    let dir = args.next().expect("usage: salvage_deep <dir>");
    let blobs = blobs_in(&dir);
    eprintln!("[{} blobs in {dir}]", blobs.len());

    let mut ok = 0usize;
    let mut err = 0usize;
    let mut total = String::new();
    for (path, bytes) in &blobs {
        let mut decoder = DecoderV1::from(bytes.as_slice());
        let mut strings = Vec::new();
        match extract_strings(&mut decoder, &mut strings) {
            Ok(()) => {
                ok += 1;
                for s in strings {
                    total.push_str(&s);
                }
            }
            Err(e) => {
                err += 1;
                eprintln!(
                    "  {}: decode error: {e:?}",
                    path.file_name().unwrap().to_string_lossy()
                );
            }
        }
    }
    eprintln!(
        "[decoded {ok} blobs, {err} errors; salvaged {} chars]",
        total.chars().count()
    );
    println!("{total}");
}
