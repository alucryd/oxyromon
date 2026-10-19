//! Unpacking: copying a CIA's parts out, undecrypted, the way ctrtool's
//! `--plain` mode does.

use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::cia::{Cia, Section, copy_range};
use crate::error::Result;

/// Which parts of the container to write besides the contents, which are
/// always written.
#[derive(Debug, Clone)]
pub struct UnpackOptions {
    /// Write the ticket as `tik`.
    pub ticket: bool,
    /// Write the footer as `footer`.
    pub footer: bool,
}

impl Default for UnpackOptions {
    fn default() -> Self {
        UnpackOptions {
            ticket: true,
            footer: true,
        }
    }
}

/// Which part of the container an extracted file came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Part {
    Certificates,
    Ticket,
    TitleMetaData,
    /// One content of the title, with its record.
    Content(crate::Content),
    Footer,
}

impl Part {
    /// The file name this part is unpacked to, in the output directory.
    pub fn name(&self) -> String {
        match self {
            Part::Certificates => "certs".into(),
            Part::Ticket => "tik".into(),
            Part::TitleMetaData => "tmd".into(),
            Part::Content(content) => {
                format!("content.{:04x}.{:08x}", content.index, content.id)
            }
            Part::Footer => "footer".into(),
        }
    }
}

/// One file `unpack` wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Extraction {
    pub path: PathBuf,
    pub part: Part,
}

/// Copy every part of the CIA at `input` into `output_dir`, undecrypted.
///
/// The files are named `certs`, `tik`, `tmd`, `content.<index>.<id>` and
/// `footer` — the names ctrtool's `--certs`, `--tmd` and `--contents` options
/// spelled, and the only spelling worth keeping: dumps matched against DATs
/// are named by what their bytes hash to, not by these files. Parts are
/// written as `<name>.part` and moved into place once every one is complete,
/// so an existing set is only replaced by a complete one, and a failed run
/// leaves nothing behind.
///
/// `progress` is called with the input bytes consumed since the last call —
/// seek gaps included, since no later read covers them — and the calls add
/// up to the input's size. The parts are returned in file order.
pub fn unpack(
    input: &Path,
    output_dir: &Path,
    options: &UnpackOptions,
    progress: &mut dyn FnMut(u64),
) -> Result<Vec<Extraction>> {
    let mut file = File::open(input)?;
    let length = file.metadata()?.len();
    let cia = Cia::read(&mut file, &mut |_| {})?;

    // Plan every file before writing any of them. The walk below is
    // monotone: each part sits past the last in the file.
    let mut plan: Vec<(Part, u64, u64, PathBuf)> = Vec::new();
    if let Some((at, size)) = cia.section(Section::Certificates) {
        plan.push((Part::Certificates, at, size, output_dir.join("certs")));
    }
    if options.ticket
        && let Some((at, size)) = cia.section(Section::Ticket)
    {
        plan.push((Part::Ticket, at, size, output_dir.join("tik")));
    }
    if let Some((at, size)) = cia.section(Section::TitleMetaData) {
        plan.push((Part::TitleMetaData, at, size, output_dir.join("tmd")));
    }
    // Contents are listed by index, but sit packed in TMD order; the walk
    // follows the file.
    let mut contents: Vec<crate::Content> = cia.contents().to_vec();
    contents.sort_by_key(|content| content.offset);
    for content in contents {
        let name = Part::Content(content).name();
        plan.push((
            Part::Content(content),
            content.offset,
            content.size,
            output_dir.join(name),
        ));
    }
    if options.footer
        && let Some((at, size)) = cia.section(Section::Footer)
    {
        plan.push((Part::Footer, at, size, output_dir.join("footer")));
    }
    debug_assert!(
        plan.windows(2)
            .all(|pair| pair[0].1 + pair[0].2 <= pair[1].1)
    );

    std::fs::create_dir_all(output_dir)?;

    let result = (|| -> Result<()> {
        let mut cursor = 0;
        for (_part, at, size, path) in &plan {
            let mut output = File::create(with_extension_part(path))?;
            cursor = copy_range(&mut file, cursor, *at, *size, &mut output, progress)?;
            output.flush()?;
        }
        // Whatever trails the last part was skipped, and is accounted for.
        if cursor < length {
            progress(length - cursor);
        }
        Ok(())
    })();

    if let Err(error) = result {
        for (_, _, _, path) in &plan {
            let _ = std::fs::remove_file(with_extension_part(path));
        }
        return Err(error);
    }

    let mut extracted = Vec::with_capacity(plan.len());
    for (part, _, _, path) in plan {
        std::fs::rename(with_extension_part(&path), &path)?;
        extracted.push(Extraction { part, path });
    }
    Ok(extracted)
}

/// `<path>.part`, byte-safely.
fn with_extension_part(path: &Path) -> PathBuf {
    let mut part = path.as_os_str().to_os_string();
    part.push(".part");
    PathBuf::from(part)
}
