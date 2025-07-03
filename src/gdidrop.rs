use super::common::*;
use super::mimetype::*;
use super::progress::*;
use indicatif::ProgressBar;
use simple_error::SimpleResult;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::fs::{File, OpenOptions};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncSeekExt, AsyncWriteExt, BufReader, SeekFrom};

#[derive(Clone)]
pub struct GdiRomfile {
    pub gdi_romfile: CommonRomfile,
    pub track_romfiles: Vec<CommonRomfile>,
}

pub trait AsGdi {
    fn as_gdi(self, track_romfiles: Vec<CommonRomfile>) -> SimpleResult<GdiRomfile>;
}

pub trait ToGdi {
    async fn to_gdi<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        destination_directory: &P,
    ) -> SimpleResult<GdiRomfile>;
}

/// CD sector size in bytes
const SECTOR_SIZE: usize = 2352;

/// CUE sheet parser and GDI converter
#[derive(Debug, Clone)]
pub struct CueSheet {
    pub tracks: Vec<Track>,
    pub catalog: Option<String>,
    pub performer: Option<String>,
    pub songwriter: Option<String>,
    pub title: Option<String>,
    pub comments: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Track {
    pub number: u32,
    pub data_type: DataType,
    pub data_file: AudioFile,
    pub indices: Vec<Index>,
    pub performer: Option<String>,
    pub songwriter: Option<String>,
    pub title: Option<String>,
    pub comments: Vec<String>,
    pub pregap: Option<Index>,
    pub postgap: Option<Index>,
}

#[derive(Debug, Clone)]
pub struct AudioFile {
    pub filename: String,
    pub file_type: FileType,
}

#[derive(Debug, Clone)]
pub struct Index {
    pub number: u32,
    pub minutes: u32,
    pub seconds: u32,
    pub frames: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DataType {
    Audio,
    Mode1_2048,
    Mode1_2352,
    Mode2_2336,
    Mode2_2352,
    Cdg,
    Cdi2336,
    Cdi2352,
}

#[derive(Debug, Clone)]
pub enum FileType {
    Binary,
    Motorola,
    Aiff,
    Wave,
    Mp3,
}

impl CueSheet {
    /// Parse a CUE sheet from a file
    pub async fn from_file<P: AsRef<Path>>(path: P) -> SimpleResult<Self> {
        let file = File::open(path)
            .await
            .map_err(|e| simple_error!("Failed to open file: {}", e))?;
        let reader = BufReader::new(file);
        let mut lines = Vec::new();
        let mut line_reader = reader.lines();

        while let Some(line) = line_reader
            .next_line()
            .await
            .map_err(|e| simple_error!("Failed to read line: {}", e))?
        {
            let trimmed = line.trim().to_string();
            if !trimmed.is_empty() {
                lines.push(trimmed);
            }
        }

        Self::parse_cue(&mut lines)
    }

    /// Parse CUE sheet from lines
    fn parse_cue(lines: &mut [String]) -> SimpleResult<Self> {
        let mut cue_sheet = CueSheet {
            tracks: Vec::new(),
            catalog: None,
            performer: None,
            songwriter: None,
            title: None,
            comments: Vec::new(),
        };

        let mut current_track: Option<Track> = None;
        let mut current_file = AudioFile {
            filename: String::new(),
            file_type: FileType::Binary,
        };

        for line in lines {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.is_empty() {
                continue;
            }

            match parts[0].to_uppercase().as_str() {
                "CATALOG" => {
                    if parts.len() > 1 {
                        cue_sheet.catalog = Some(parts[1].to_string());
                    }
                }
                "PERFORMER" => {
                    let performer = Self::extract_quoted_string(line);
                    if current_track.is_some() {
                        current_track.as_mut().unwrap().performer = Some(performer);
                    } else {
                        cue_sheet.performer = Some(performer);
                    }
                }
                "SONGWRITER" => {
                    let songwriter = Self::extract_quoted_string(line);
                    if current_track.is_some() {
                        current_track.as_mut().unwrap().songwriter = Some(songwriter);
                    } else {
                        cue_sheet.songwriter = Some(songwriter);
                    }
                }
                "TITLE" => {
                    let title = Self::extract_quoted_string(line);
                    if current_track.is_some() {
                        current_track.as_mut().unwrap().title = Some(title);
                    } else {
                        cue_sheet.title = Some(title);
                    }
                }
                "FILE" => {
                    current_file = Self::parse_file(line)?;
                }
                "TRACK" => {
                    // Save previous track
                    if let Some(track) = current_track.take() {
                        cue_sheet.tracks.push(track);
                    }

                    // Create new track
                    if parts.len() >= 3 {
                        let track_number: u32 = parts[1]
                            .parse()
                            .map_err(|e| simple_error!("Failed to parse track number: {}", e))?;
                        let data_type = Self::parse_data_type(parts[2])?;

                        current_track = Some(Track {
                            number: track_number,
                            data_type,
                            data_file: current_file.clone(),
                            indices: Vec::new(),
                            performer: None,
                            songwriter: None,
                            title: None,
                            comments: Vec::new(),
                            pregap: None,
                            postgap: None,
                        });
                    }
                }
                "INDEX" => {
                    if let Some(ref mut track) = current_track {
                        if parts.len() >= 3 {
                            let index = Self::parse_index(parts[1], parts[2])?;
                            track.indices.push(index);
                        }
                    }
                }
                "PREGAP" => {
                    if let Some(ref mut track) = current_track {
                        if parts.len() >= 2 {
                            let index = Self::parse_index("0", parts[1])?;
                            track.pregap = Some(index);
                        }
                    }
                }
                "POSTGAP" => {
                    if let Some(ref mut track) = current_track {
                        if parts.len() >= 2 {
                            let index = Self::parse_index("0", parts[1])?;
                            track.postgap = Some(index);
                        }
                    }
                }
                "REM" => {
                    let comment = line.strip_prefix("REM").unwrap_or("").trim().to_string();
                    if !comment.is_empty() {
                        if let Some(ref mut track) = current_track {
                            track.comments.push(comment);
                        } else {
                            cue_sheet.comments.push(comment);
                        }
                    }
                }
                _ => {} // Ignore unknown commands
            }
        }

        // Save last track
        if let Some(track) = current_track {
            cue_sheet.tracks.push(track);
        }

        Ok(cue_sheet)
    }

    fn extract_quoted_string(line: &str) -> String {
        if let Some(start) = line.find('"') {
            if let Some(end) = line.rfind('"') {
                if start < end {
                    return line[start + 1..end].to_string();
                }
            }
        }
        // Fallback: take everything after the first space
        line.split_whitespace()
            .skip(1)
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn parse_file(line: &str) -> SimpleResult<AudioFile> {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 3 {
            bail!("Invalid FILE command: {}", line);
        }

        let filename = Self::extract_quoted_string(line);
        let file_type = match parts.last().unwrap().to_uppercase().as_str() {
            "BINARY" => FileType::Binary,
            "MOTOROLA" => FileType::Motorola,
            "AIFF" => FileType::Aiff,
            "WAVE" => FileType::Wave,
            "MP3" => FileType::Mp3,
            _ => FileType::Binary,
        };

        Ok(AudioFile {
            filename,
            file_type,
        })
    }

    fn parse_data_type(data_type_str: &str) -> SimpleResult<DataType> {
        match data_type_str.to_uppercase().as_str() {
            "AUDIO" => Ok(DataType::Audio),
            "MODE1/2048" => Ok(DataType::Mode1_2048),
            "MODE1/2352" => Ok(DataType::Mode1_2352),
            "MODE2/2336" => Ok(DataType::Mode2_2336),
            "MODE2/2352" => Ok(DataType::Mode2_2352),
            "CDG" => Ok(DataType::Cdg),
            "CDI/2336" => Ok(DataType::Cdi2336),
            "CDI/2352" => Ok(DataType::Cdi2352),
            _ => Ok(DataType::Audio), // Default
        }
    }

    fn parse_index(number_str: &str, time_str: &str) -> SimpleResult<Index> {
        let number: u32 = number_str
            .parse()
            .map_err(|e| simple_error!("Failed to parse index number: {}", e))?;
        let time_parts: Vec<&str> = time_str.split(':').collect();

        if time_parts.len() != 3 {
            bail!("Invalid time format: {}", time_str);
        }

        let minutes: u32 = time_parts[0]
            .parse()
            .map_err(|e| simple_error!("Failed to parse minutes: {}", e))?;
        let seconds: u32 = time_parts[1]
            .parse()
            .map_err(|e| simple_error!("Failed to parse seconds: {}", e))?;
        let frames: u32 = time_parts[2]
            .parse()
            .map_err(|e| simple_error!("Failed to parse frames: {}", e))?;

        Ok(Index {
            number,
            minutes,
            seconds,
            frames,
        })
    }

    /// Convert CUE/BIN to GDI format
    pub async fn convert_to_gdi<P: AsRef<Path>>(
        &self,
        working_directory: P,
        destination_directory: P,
    ) -> SimpleResult<(String, Vec<PathBuf>)> {
        let mut current_sector = 0;
        let mut gdi_output = String::new();
        let mut track_paths = Vec::new();

        // Write track count
        gdi_output.push_str(&format!("{}\n", self.tracks.len()));

        for track in &self.tracks {
            let input_track_path = working_directory.as_ref().join(&track.data_file.filename);
            let can_perform_full_copy = track.indices.len() == 1;

            let input_filename = Path::new(&track.data_file.filename)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("track");

            let extension = if track.data_type == DataType::Audio {
                RAW_EXTENSION
            } else {
                BIN_EXTENSION
            };

            let output_filename = format!("{}.{}", input_filename, extension);
            let output_track_path = destination_directory.as_ref().join(&output_filename);

            let sector_amount = if can_perform_full_copy {
                self.copy_full_file(&input_track_path, &output_track_path)
                    .await?
            } else {
                let gap_offset = if track.indices.len() > 1 {
                    self.count_index_frames(&track.indices[1])
                } else {
                    0
                };
                let sectors = self
                    .copy_file_with_gap_offset(&input_track_path, &output_track_path, gap_offset)
                    .await?;
                current_sector += gap_offset;
                sectors
            };

            let gap = 0; // Placeholder for gap value
            let track_type = if track.data_type == DataType::Audio {
                "0"
            } else {
                "4"
            };

            gdi_output.push_str(&format!(
                "{} {} {} {} \"{}\" {}\n",
                track.number, current_sector, track_type, SECTOR_SIZE, output_filename, gap
            ));

            current_sector += sector_amount;

            // Check for HIGH-DENSITY AREA comment
            if track
                .comments
                .iter()
                .any(|c| c.contains("HIGH-DENSITY AREA"))
            {
                if current_sector < 45000 {
                    current_sector = 45000;
                }
            }

            track_paths.push(output_track_path);
        }

        Ok((gdi_output, track_paths))
    }

    async fn copy_full_file<P: AsRef<Path>>(
        &self,
        input_path: P,
        output_path: P,
    ) -> SimpleResult<u32> {
        tokio::fs::copy(&input_path, &output_path)
            .await
            .map_err(|e| simple_error!("Failed to copy file: {}", e))?;
        let metadata = tokio::fs::metadata(&input_path)
            .await
            .map_err(|e| simple_error!("Failed to get metadata: {}", e))?;
        let file_size = metadata.len();
        Ok((file_size / SECTOR_SIZE as u64) as u32)
    }

    async fn copy_file_with_gap_offset<P: AsRef<Path>>(
        &self,
        input_path: P,
        output_path: P,
        frames: u32,
    ) -> SimpleResult<u32> {
        let mut input_file = File::open(&input_path)
            .await
            .map_err(|e| simple_error!("Failed to open input file: {}", e))?;
        let mut output_file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&output_path)
            .await
            .map_err(|e| simple_error!("Failed to open output file: {}", e))?;

        // Skip gap frames
        let skip_bytes = frames as u64 * SECTOR_SIZE as u64;
        input_file
            .seek(SeekFrom::Start(skip_bytes))
            .await
            .map_err(|e| simple_error!("Failed to seek: {}", e))?;

        // Calculate remaining sectors
        let metadata = input_file
            .metadata()
            .await
            .map_err(|e| simple_error!("Failed to get metadata: {}", e))?;
        let total_size = metadata.len();
        let remaining_size = total_size - skip_bytes;
        let sector_count = (remaining_size / SECTOR_SIZE as u64) as u32;

        // Copy data in sector-sized chunks
        let mut buffer = vec![0u8; SECTOR_SIZE];
        let mut _copied_sectors = 0;

        loop {
            let bytes_read = input_file
                .read(&mut buffer)
                .await
                .map_err(|e| simple_error!("Failed to read: {}", e))?;
            if bytes_read == 0 {
                break;
            }
            output_file
                .write_all(&buffer[..bytes_read])
                .await
                .map_err(|e| simple_error!("Failed to write: {}", e))?;
            if bytes_read == SECTOR_SIZE {
                _copied_sectors += 1;
            }
        }

        output_file
            .flush()
            .await
            .map_err(|e| simple_error!("Failed to flush: {}", e))?;
        Ok(sector_count)
    }

    fn count_index_frames(&self, index: &Index) -> u32 {
        index.frames + (index.seconds * 75) + (index.minutes * 60 * 75)
    }
}

// === GDI TRAIT IMPLEMENTATIONS ===

impl AsGdi for CommonRomfile {
    fn as_gdi(self, track_romfiles: Vec<CommonRomfile>) -> SimpleResult<GdiRomfile> {
        if self
            .path
            .extension()
            .unwrap()
            .to_str()
            .unwrap()
            .to_lowercase()
            != GDI_EXTENSION
        {
            bail!("Not a valid gdi");
        }
        // Validate track files (can be .bin, .raw, or other valid extensions)
        for track_romfile in &track_romfiles {
            let extension = track_romfile
                .path
                .extension()
                .unwrap()
                .to_str()
                .unwrap()
                .to_lowercase();
            if extension != BIN_EXTENSION && extension != RAW_EXTENSION {
                bail!("Not a valid track file extension: {}", extension);
            }
        }
        Ok(GdiRomfile {
            gdi_romfile: self,
            track_romfiles,
        })
    }
}

impl ToGdi for CueBinRomfile {
    async fn to_gdi<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        destination_directory: &P,
    ) -> SimpleResult<GdiRomfile> {
        progress_bar.set_message("Converting CUE/BIN to GDI");
        progress_bar.set_style(get_none_progress_style());
        progress_bar.enable_steady_tick(Duration::from_millis(100));

        // Parse the CUE file to understand the track structure
        let cue_sheet = CueSheet::from_file(&self.cue_romfile.path).await?;

        // Convert to GDI format
        let (gdi_content, track_paths) = cue_sheet
            .convert_to_gdi(
                self.cue_romfile.path.parent().unwrap(),
                destination_directory.as_ref(),
            )
            .await?;

        // Create the GDI file
        let gdi_path = destination_directory
            .as_ref()
            .join(self.cue_romfile.path.file_name().unwrap())
            .with_extension(GDI_EXTENSION);
        tokio::fs::write(&gdi_path, gdi_content)
            .await
            .map_err(|e| simple_error!("Failed to write GDI file: {}", e))?;

        // Collect the track romfiles that were created by the conversion
        let track_romfiles = track_paths
            .into_iter()
            .map(|path| CommonRomfile::from_path(&path))
            .collect::<SimpleResult<Vec<CommonRomfile>>>()?;

        progress_bar.set_message("");
        progress_bar.disable_steady_tick();

        Ok(GdiRomfile {
            gdi_romfile: CommonRomfile::from_path(&gdi_path)?,
            track_romfiles,
        })
    }
}
