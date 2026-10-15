use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::path::{Path, PathBuf};

use mp4::{AvcConfig, MediaConfig, Mp4Config, Mp4Reader, Mp4Writer, TrackConfig, TrackType};

type Error = Box<dyn std::error::Error + Send + Sync>;

fn open(path: &Path) -> Result<Mp4Reader<BufReader<File>>, Error> {
    let f = File::open(path)?;
    let len = f.metadata()?.len();
    Ok(Mp4Reader::read_header(BufReader::new(f), len)?)
}

fn video_track_id(r: &Mp4Reader<BufReader<File>>) -> Result<u32, Error> {
    for (id, t) in r.tracks() {
        if t.track_type()? == TrackType::Video {
            return Ok(*id);
        }
    }
    Err("no video track".into())
}

/// Copies the video samples of every file, in order, into one mp4 without
/// re-encoding. Returns how long each input was, in seconds.
pub fn join(files: &[PathBuf], out: &Path) -> Result<Vec<f64>, Error> {
    let first = files.first().ok_or("nothing to join")?;
    let r = open(first)?;
    let id = video_track_id(&r)?;
    let track = &r.tracks()[&id];
    let scale = track.timescale();

    let cfg = Mp4Config {
        major_brand: *r.major_brand(),
        minor_version: r.minor_version(),
        compatible_brands: r.compatible_brands().to_vec(),
        timescale: r.timescale(),
    };
    let mut w = Mp4Writer::write_start(BufWriter::new(File::create(out)?), &cfg)?;
    w.add_track(&TrackConfig {
        track_type: TrackType::Video,
        timescale: scale,
        language: "und".into(),
        media_conf: MediaConfig::AvcConfig(AvcConfig {
            width: track.width(),
            height: track.height(),
            seq_param_set: track.sequence_parameter_set()?.to_vec(),
            pic_param_set: track.picture_parameter_set()?.to_vec(),
        }),
    })?;
    drop(r);

    let mut offset = 0u64;
    let mut lens = Vec::new();

    for f in files {
        let mut r = open(f)?;
        let id = video_track_id(&r)?;
        if r.tracks()[&id].timescale() != scale {
            return Err(format!("timescale mismatch in {}", f.display()).into());
        }

        let n = r.sample_count(id)?;
        let mut base = None;
        let mut end = 0;
        for i in 1..=n {
            let Some(mut s) = r.read_sample(id, i)? else { continue };
            let b = *base.get_or_insert(s.start_time);
            end = s.start_time - b + s.duration as u64;
            s.start_time = offset + (s.start_time - b);
            w.write_sample(1, &s)?;
        }
        offset += end;
        lens.push(end as f64 / scale as f64);
    }

    w.write_end()?;
    Ok(lens)
}
