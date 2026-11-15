use crate::{
    Error, NodeId, Result, cleanup,
    exports::{StreamOutcome, save_generated_file},
    index::Index,
    operations::Lease,
    windows::{
        self,
        cleanup_io::{LeafAccess, open_leaf},
    },
};
use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    io::{Read, Seek},
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};
use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;

const MIB: u64 = 1024 * 1024;
const CANCELLED: &str = "Video compression cancelled.";
const ENCODE_TIMEOUT: Duration = Duration::from_secs(4 * 3600);
const MEDIA_INPUT: &[&str] = &[
    "-protocol_whitelist",
    "fd",
    "-f",
    "mov",
    "-enable_drefs",
    "0",
    "-probesize",
    "16777216",
    "-analyzeduration",
    "10000000",
];

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VideoPhase {
    Checking,
    Encoding,
    Verifying,
}

#[derive(Clone, Copy)]
enum Encoder {
    Hardware,
    Software,
}

impl Encoder {
    fn args(self) -> &'static [&'static str] {
        match self {
            Self::Hardware => &["-c:v", "h264_mf", "-hw_encoding", "1", "-pix_fmt", "nv12"],
            Self::Software => &[
                "-c:v",
                "libx264",
                "-preset",
                "fast",
                "-pix_fmt",
                "yuv420p",
                "-threads:v",
                "4",
            ],
        }
    }
}

#[derive(Clone, Copy, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum VideoResult {
    Cancelled,
    NoReduction,
    Published {
        original: u64,
        encoded: u64,
        width: u32,
        height: u32,
        duration_ms: u64,
        hardware: bool,
    },
}

struct Toolchain {
    ffmpeg: PathBuf,
    ffprobe: PathBuf,
    _directory: crate::agents::AuthorizedPath,
    _binaries: [File; 2],
}

impl Toolchain {
    fn open(path: &Path) -> Result<Self> {
        let path = crate::agents::ordinary_path(path)?;
        if !path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.eq_ignore_ascii_case("ffmpeg.exe"))
        {
            return Err(message(
                "Select ffmpeg.exe from a trusted FFmpeg installation.",
            ));
        }
        let directory = crate::agents::pin_directory(
            path.parent()
                .ok_or_else(|| message("Choose the FFmpeg installation folder."))?,
        )?;
        let bin = |name: &str| -> Result<File> {
            let file = open_leaf(
                directory.directory_handle()?,
                name.as_ref(),
                LeafAccess::Retain,
            )
            .map_err(|_| message("FFmpeg and ffprobe must be in the same local folder."))?;
            let metadata = file.metadata()?;
            if !metadata.is_file()
                || windows::is_reparse(&metadata)
                || windows::requires_recall(&metadata)
            {
                return Err(message("FFmpeg and ffprobe must be ordinary local files."));
            }
            Ok(file)
        };
        let binaries = [bin("ffmpeg.exe")?, bin("ffprobe.exe")?];
        Ok(Self {
            ffmpeg: directory.path().join("ffmpeg.exe"),
            ffprobe: directory.path().join("ffprobe.exe"),
            _directory: directory,
            _binaries: binaries,
        })
    }

    fn ffmpeg(&self) -> Command {
        let mut command = Self::command(&self.ffmpeg);
        command.args(["-hide_banner", "-loglevel", "error", "-nostdin", "-xerror"]);
        command
    }

    fn command(path: &Path) -> Command {
        let mut command = Command::new(path);
        if let Some(directory) = path.parent() {
            command.current_dir(directory);
        }
        command
            .creation_flags(CREATE_NO_WINDOW)
            .env_remove("FFREPORT")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        command
    }

    fn can_encode(&self, encoder: Encoder, cancel: &AtomicBool) -> Result<bool> {
        let mut command = self.ffmpeg();
        command
            .args([
                "-f",
                "lavfi",
                "-i",
                "color=size=128x128:rate=30",
                "-frames:v",
                "2",
            ])
            .args(encoder.args())
            .args(["-b:v", "200000", "-f", "null", "-"]);
        let outcome = run(&mut command.spawn()?, cancel, Duration::from_secs(12), None)?;
        Ok(outcome == Run::Success)
    }

    fn encode(
        &self,
        mut input: &File,
        mut output: &File,
        original_size: u64,
        bitrate: u64,
        encoder: Encoder,
        cancel: &AtomicBool,
    ) -> Result<Run> {
        check_cancel(cancel)?;
        input.rewind()?;
        output.set_len(0)?;
        output.rewind()?;
        let bitrate = bitrate.clamp(100_000, 20_000_000).to_string();
        let mut child = self
            .ffmpeg()
            .args(MEDIA_INPUT)
            .args([
                "-threads",
                "4",
                "-i",
                "fd:",
                "-map",
                "0:v:0",
                "-map",
                "0:a:0?",
                "-map_metadata",
                "-1",
                "-map_chapters",
                "-1",
                "-c:a",
                "copy",
                "-fps_mode",
                "passthrough",
                "-b:v",
                &bitrate,
                "-f",
                "mp4",
            ])
            .args(encoder.args())
            .arg("fd:")
            .stdin(Stdio::from(input.try_clone()?))
            .stdout(Stdio::from(output.try_clone()?))
            .spawn()?;
        run(
            &mut child,
            cancel,
            ENCODE_TIMEOUT,
            Some((output, original_size)),
        )
    }

    fn probe(&self, mut input: &File, cancel: &AtomicBool) -> Result<Media> {
        input.rewind()?;
        let mut child = Self::command(&self.ffprobe)
            .args(["-v", "error"]).args(MEDIA_INPUT)
            .args(["-show_entries", "format=duration:stream=codec_type,codec_name,width,height,pix_fmt,color_transfer,channels,nb_frames,sample_aspect_ratio:stream_disposition=attached_pic:stream_side_data=rotation",
                "-of", "json", "-i", "fd:"])
            .stdin(Stdio::from(input.try_clone()?)).stdout(Stdio::piped()).spawn()?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| message("Cannot read the video probe."))?;
        let reader = std::thread::spawn(move || {
            let mut bytes = Vec::new();
            stdout.take(65_537).read_to_end(&mut bytes)?;
            Ok::<_, std::io::Error>(bytes)
        });
        let outcome = run(&mut child, cancel, Duration::from_secs(20), None);
        let bytes = reader
            .join()
            .map_err(|_| message("Video probe stopped unexpectedly."))??;
        if outcome? != Run::Success || bytes.len() > 65_536 {
            return Err(message(
                "FFprobe could not read the local MP4 or MOV. The build must support the fd protocol.",
            ));
        }
        let report: Probe =
            serde_json::from_slice(&bytes).map_err(|_| message("Unsupported video metadata."))?;
        report.media()
    }
}

#[derive(Deserialize)]
struct Probe {
    streams: Vec<Stream>,
    format: Format,
}

#[derive(Deserialize)]
struct Format {
    duration: String,
}

#[derive(Deserialize)]
struct Stream {
    codec_type: String,
    codec_name: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    pix_fmt: Option<String>,
    color_transfer: Option<String>,
    channels: Option<u32>,
    nb_frames: Option<String>,
    sample_aspect_ratio: Option<String>,
    disposition: Option<Disposition>,
    #[serde(default)]
    side_data_list: Vec<serde_json::Value>,
}

#[derive(Deserialize)]
struct Disposition {
    attached_pic: u8,
}

struct Media {
    dimensions: (u32, u32),
    seconds: f64,
    audio: Option<u32>,
    h264: bool,
    frames: Option<u64>,
    aspect: String,
}

impl Probe {
    fn media(self) -> Result<Media> {
        let unsupported = || {
            message(
                "Use an 8-bit SDR MP4 or MOV with one video track and at most one AAC audio track.",
            )
        };
        let mut video = None;
        let mut audio = None;
        for stream in self.streams {
            match stream.codec_type.as_str() {
                "video" if video.is_none() => video = Some(stream),
                "audio" if audio.is_none() && stream.codec_name.as_deref() == Some("aac") => {
                    audio = Some(
                        stream
                            .channels
                            .filter(|n| (1..=8).contains(n))
                            .ok_or_else(unsupported)?,
                    );
                }
                _ => return Err(unsupported()),
            }
        }
        let video = video.ok_or_else(unsupported)?;
        if !matches!(video.pix_fmt.as_deref(), Some("yuv420p" | "nv12"))
            || matches!(
                video.color_transfer.as_deref(),
                Some("smpte2084" | "arib-std-b67")
            )
            || video
                .disposition
                .is_some_and(|value| value.attached_pic != 0)
            || video
                .side_data_list
                .iter()
                .any(|entry| entry.get("rotation").is_some())
        {
            return Err(unsupported());
        }
        let dimension = |value: Option<u32>| {
            value
                .filter(|n| (128..=4096).contains(n) && n % 2 == 0)
                .ok_or_else(unsupported)
        };
        let seconds = self
            .format
            .duration
            .parse::<f64>()
            .ok()
            .filter(|seconds| (1.0..=14_400.0).contains(seconds))
            .ok_or_else(unsupported)?;
        let aspect = video
            .sample_aspect_ratio
            .filter(|ratio| ratio != "N/A" && ratio != "0:1");
        Ok(Media {
            dimensions: (dimension(video.width)?, dimension(video.height)?),
            seconds,
            audio,
            h264: video.codec_name.as_deref() == Some("h264"),
            frames: video.nb_frames.and_then(|count| count.parse().ok()),
            aspect: aspect.unwrap_or_else(|| "1:1".into()),
        })
    }
}

pub struct VideoRequest<'a> {
    pub index: &'a Index,
    pub id: NodeId,
    pub encoder_path: &'a Path,
    pub destination: &'a Path,
}

pub fn compress_video(
    request: VideoRequest<'_>,
    lease: &Lease,
    cancel: &AtomicBool,
    mut phase: impl FnMut(VideoPhase),
) -> Result<VideoResult> {
    let result = compress(request, lease, cancel, &mut phase);
    if matches!(&result, Err(Error::Invalid(reason)) if reason == CANCELLED) {
        return Ok(VideoResult::Cancelled);
    }
    result
}

fn compress(
    request: VideoRequest<'_>,
    lease: &Lease,
    cancel: &AtomicBool,
    phase: &mut impl FnMut(VideoPhase),
) -> Result<VideoResult> {
    let VideoRequest {
        index,
        id,
        encoder_path,
        destination,
    } = request;
    check_cancel(cancel)?;
    let entry = index.entry(id)?;
    let path = index.path(id)?;
    let supported = path
        .extension()
        .and_then(|name| name.to_str())
        .is_some_and(|value| {
            value.eq_ignore_ascii_case("mp4") || value.eq_ignore_ascii_case("mov")
        });
    if !supported || !(MIB..=4096 * MIB).contains(&entry.logical) {
        return Err(message(
            "Choose a live MP4 or MOV file between 1 MiB and 4 GiB.",
        ));
    }
    let parent = super::checked_parent(index, entry, &path)?;
    let parent_ids = parent.ancestor_identities()?;
    let system = cleanup::protected_identities()?;
    let destination = crate::agents::ordinary_path(destination)?;
    let destination_parent = crate::agents::pin_directory(
        destination
            .parent()
            .ok_or_else(|| message("Choose an output folder."))?,
    )?;
    if cleanup::record::sensitive(&path)
        || parent_ids
            .iter()
            .any(|id| system.contains(id) || *id == lease.directory_identity())
        || destination_parent
            .ancestor_identities()?
            .contains(&lease.directory_identity())
    {
        return Err(message(
            "Video compression requires an unprotected file and output folder outside Voldiq storage.",
        ));
    }
    match std::fs::symlink_metadata(&destination) {
        Ok(_) => {
            return Err(message(
                "Choose a new output filename. Existing files are preserved.",
            ));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    let file = open_leaf(
        parent.directory_handle()?,
        path.file_name()
            .ok_or_else(|| message("Video filename is unavailable."))?,
        LeafAccess::Retain,
    )?;
    super::verify(&file, entry, true)?;
    let protections = || crate::metadata::Store::open(lease.directory())?.protections();
    protections()?.check_evidence(&path, windows::file_identity(&file)?.key, &parent_ids)?;
    phase(VideoPhase::Checking);
    let tools = Toolchain::open(encoder_path)?;
    let source = tools.probe(&file, cancel)?;
    let size = entry.logical;
    let outcome = save_generated_file("mp4", &destination, lease, cancel, |output| {
        let budget = (size as f64 * 8.0 / source.seconds * 0.55) as u64;
        let mut used = None;
        for encoder in [Encoder::Hardware, Encoder::Software] {
            phase(VideoPhase::Checking);
            if !tools.can_encode(encoder, cancel)? {
                continue;
            }
            phase(VideoPhase::Encoding);
            match tools.encode(&file, output, size, budget, encoder, cancel)? {
                Run::Success => {
                    used = Some(encoder);
                    break;
                }
                Run::Failed | Run::TimedOut => {}
                Run::SizeLimit => return Ok(StreamOutcome::Cancelled(VideoResult::NoReduction)),
            }
        }
        let encoder = used.ok_or_else(|| {
            message(
                "This FFmpeg installation could not compress the clip using hardware or libx264.",
            )
        })?;
        let encoded = output.metadata()?.len();
        if encoded >= size {
            return Ok(StreamOutcome::Cancelled(VideoResult::NoReduction));
        }
        phase(VideoPhase::Verifying);
        let verified = tools.probe(output, cancel)?;
        if !verified.h264
            || source.dimensions != verified.dimensions
            || source.audio != verified.audio
            || source.aspect != verified.aspect
            || source
                .frames
                .is_some_and(|frames| verified.frames != Some(frames))
            || (source.seconds - verified.seconds).abs() > 0.25
        {
            return Err(message(
                "The encoded video failed the duration, resolution or track checks.",
            ));
        }
        let mut readback = output;
        readback.rewind()?;
        let mut child = tools
            .ffmpeg()
            .args(MEDIA_INPUT)
            .args([
                "-i", "fd:", "-map", "0:v:0", "-map", "0:a:0?", "-f", "null", "-",
            ])
            .stdin(Stdio::from(output.try_clone()?))
            .spawn()?;
        if run(&mut child, cancel, ENCODE_TIMEOUT, None)? != Run::Success {
            return Err(message(
                "The encoded video could not be decoded to the end.",
            ));
        }
        super::verify(&file, entry, true)?;
        protections()?.check_evidence(&path, windows::file_identity(&file)?.key, &parent_ids)?;
        Ok(StreamOutcome::Complete(VideoResult::Published {
            original: size,
            encoded,
            width: verified.dimensions.0,
            height: verified.dimensions.1,
            duration_ms: (verified.seconds * 1000.0).round() as u64,
            hardware: matches!(encoder, Encoder::Hardware),
        }))
    })?;
    Ok(match outcome {
        StreamOutcome::Complete(receipt)
        | StreamOutcome::Cancelled(receipt @ VideoResult::NoReduction) => receipt,
        StreamOutcome::Cancelled(_) => VideoResult::Cancelled,
    })
}

#[derive(PartialEq)]
enum Run {
    Success,
    Failed,
    SizeLimit,
    TimedOut,
}

fn run(
    child: &mut Child,
    cancel: &AtomicBool,
    limit: Duration,
    output_limit: Option<(&File, u64)>,
) -> Result<Run> {
    let start = Instant::now();
    loop {
        let status = check_cancel(cancel).and_then(|()| {
            if let Some((output, size)) = output_limit
                && output.metadata()?.len() >= size
            {
                return Ok(Some(Run::SizeLimit));
            }
            if start.elapsed() >= limit {
                return Ok(Some(Run::TimedOut));
            }
            Ok(child.try_wait()?.map(|status| {
                if status.success() {
                    Run::Success
                } else {
                    Run::Failed
                }
            }))
        });
        match status {
            Ok(Some(status @ (Run::Success | Run::Failed))) => return Ok(status),
            Ok(None) => std::thread::sleep(Duration::from_millis(40)),
            Ok(Some(outcome)) => {
                let _ = child.kill();
                let _ = child.wait();
                return Ok(outcome);
            }
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
        }
    }
}

fn check_cancel(cancel: &AtomicBool) -> Result<()> {
    if cancel.load(Ordering::Acquire) {
        return Err(message(CANCELLED));
    }
    Ok(())
}

fn message(text: &str) -> Error {
    Error::Invalid(text.into())
}
