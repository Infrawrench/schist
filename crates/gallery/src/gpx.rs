//! Geotagging from GPS tracks: GPX 1.0/1.1 parsing and capture-time matching.
//!
//! A track is read namespace-agnostically (GPX 1.0 and 1.1 differ in their
//! namespace URI, not in the elements used here). Timed track points
//! (`trk/trkseg/trkpt`) are preferred; a file without any falls back to timed
//! route points, then timed waypoints. Each `trkseg` stays a separate segment:
//! a segment boundary is where the logger lost its fix or was paused, so
//! positions are never interpolated across it.
//!
//! Photo capture times are matched in UTC. A time with an explicit offset
//! (an XMP time such as `…+02:00`, or EXIF `OffsetTimeOriginal`) is absolute;
//! a bare EXIF time is local camera time and needs the user's time zone. A
//! camera clock correction is added to both.

use anyhow::{anyhow, bail, Result};
use chrono::{DateTime, NaiveDateTime};
use std::path::Path;

/// One logged position. `time` is UTC milliseconds since the Unix epoch.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrackPoint {
    pub time: Option<i64>,
    pub lat: f64,
    pub lon: f64,
    pub ele: Option<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Track,
    Route,
    Waypoints,
}

/// A continuous run of points, sorted by time when timed.
#[derive(Clone, Debug, PartialEq)]
pub struct Segment {
    pub points: Vec<TrackPoint>,
    pub source: Source,
}

impl Segment {
    fn timed(&self) -> impl Iterator<Item = (i64, &TrackPoint)> {
        self.points.iter().filter_map(|p| p.time.map(|t| (t, p)))
    }
    pub fn span(&self) -> Option<(i64, i64)> {
        let mut timed = self.timed();
        let first = timed.next()?.0;
        Some((first, timed.last().map_or(first, |(t, _)| t)))
    }
}

/// Every segment found in one or more GPX files.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Tracks {
    pub segments: Vec<Segment>,
    /// Points dropped for invalid coordinates or unreadable times.
    pub skipped_points: usize,
}

impl Tracks {
    pub fn timed_points(&self) -> usize {
        self.segments.iter().map(|s| s.timed().count()).sum()
    }
    pub fn span(&self) -> Option<(i64, i64)> {
        self.segments
            .iter()
            .filter_map(Segment::span)
            .reduce(|a, b| (a.0.min(b.0), a.1.max(b.1)))
    }
    pub fn extend(&mut self, other: Tracks) {
        self.segments.extend(other.segments);
        self.skipped_points += other.skipped_points;
        self.segments
            .sort_by_key(|s| s.span().map_or(i64::MAX, |(start, _)| start));
    }
}

/// GPX is bounded like other sidecar inputs; a day of 1 Hz logging is ~10 MB.
pub const MAX_GPX_BYTES: u64 = 256 * 1024 * 1024;

pub fn read_file(path: &Path) -> Result<Tracks> {
    let len = std::fs::metadata(path)?.len();
    if len > MAX_GPX_BYTES {
        bail!("GPX file is too large");
    }
    let bytes = std::fs::read(path)?;
    let text = String::from_utf8(bytes).map_err(|_| anyhow!("GPX file is not UTF-8"))?;
    parse(&text)
}

/// Read every file; one malformed file fails the whole set so the user is not
/// shown a preview silently missing a day.
pub fn read_files(paths: &[impl AsRef<Path>]) -> Result<Tracks> {
    let mut all = Tracks::default();
    for path in paths {
        let path = path.as_ref();
        let tracks = read_file(path).map_err(|e| anyhow!("{}: {e:#}", path.display()))?;
        all.extend(tracks);
    }
    Ok(all)
}

/// xsd:dateTime. GPX specifies UTC; a zoneless value is read as UTC.
pub fn parse_gpx_time(value: &str) -> Option<i64> {
    let value = value.trim();
    if let Ok(time) = DateTime::parse_from_rfc3339(value) {
        return Some(time.timestamp_millis());
    }
    NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M:%S%.f")
        .ok()
        .map(|t| t.and_utc().timestamp_millis())
}

pub fn parse(text: &str) -> Result<Tracks> {
    let doc = roxmltree::Document::parse(text)?; // DTDs are refused by default.
    let root = doc.root_element();
    if root.tag_name().name() != "gpx" {
        bail!("not a GPX document");
    }
    let mut skipped = 0usize;
    let mut point = |node: roxmltree::Node| -> Option<TrackPoint> {
        let coordinate = |name: &str| -> Option<f64> {
            node.attribute(name)?
                .trim()
                .parse::<f64>()
                .ok()
                .filter(|v| v.is_finite())
        };
        let child = |name: &str| {
            node.children()
                .find(|c| c.is_element() && c.tag_name().name() == name)
                .and_then(|c| c.text())
        };
        let (Some(lat), Some(lon)) = (coordinate("lat"), coordinate("lon")) else {
            skipped += 1;
            return None;
        };
        if !(-90.0..=90.0).contains(&lat) || !(-180.0..=180.0).contains(&lon) {
            skipped += 1;
            return None;
        }
        let time = match child("time") {
            Some(text) => match parse_gpx_time(text) {
                Some(time) => Some(time),
                None => {
                    skipped += 1;
                    return None;
                }
            },
            None => None,
        };
        let ele = child("ele")
            .and_then(|e| e.trim().parse::<f64>().ok())
            .filter(|e| e.is_finite());
        Some(TrackPoint {
            time,
            lat,
            lon,
            ele,
        })
    };
    fn elements<'a, 'i>(node: roxmltree::Node<'a, 'i>, name: &str) -> Vec<roxmltree::Node<'a, 'i>> {
        node.children()
            .filter(|c| c.is_element() && c.tag_name().name() == name)
            .collect()
    }
    let mut tracks = Vec::new();
    for trk in elements(root, "trk") {
        for seg in elements(trk, "trkseg") {
            let points: Vec<_> = elements(seg, "trkpt")
                .into_iter()
                .filter_map(&mut point)
                .collect();
            tracks.push(Segment {
                points,
                source: Source::Track,
            });
        }
    }
    let mut routes = Vec::new();
    for rte in elements(root, "rte") {
        let points: Vec<_> = elements(rte, "rtept")
            .into_iter()
            .filter_map(&mut point)
            .collect();
        routes.push(Segment {
            points,
            source: Source::Route,
        });
    }
    let waypoints: Vec<_> = elements(root, "wpt")
        .into_iter()
        .filter_map(&mut point)
        .collect();
    let timed = |segments: &[Segment]| segments.iter().any(|s| s.span().is_some());
    let mut segments = if timed(&tracks) {
        tracks
    } else if timed(&routes) {
        routes
    } else if waypoints.iter().any(|p| p.time.is_some()) {
        vec![Segment {
            points: waypoints,
            source: Source::Waypoints,
        }]
    } else {
        // Untimed geometry can still be drawn, but nothing will match.
        tracks.into_iter().chain(routes).collect()
    };
    for segment in &mut segments {
        normalize(segment);
    }
    segments.retain(|s| !s.points.is_empty());
    let mut result = Tracks {
        segments: Vec::new(),
        skipped_points: skipped,
    };
    result.extend(Tracks {
        segments,
        skipped_points: 0,
    });
    Ok(result)
}

/// Time-order timed points and drop repeated timestamps (loggers write them
/// when paused); untimed segments keep their drawing order.
fn normalize(segment: &mut Segment) {
    if segment.points.iter().all(|p| p.time.is_none()) {
        return;
    }
    segment.points.retain(|p| p.time.is_some());
    segment.points.sort_by_key(|p| p.time);
    segment.points.dedup_by_key(|p| p.time);
}

/// A photo's capture time as recorded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureTime {
    /// UTC milliseconds: the recorded time carried an offset.
    Absolute(i64),
    /// Camera-local wall clock, as milliseconds of a UTC-less calendar.
    Local(i64),
}

/// Parse an ISO time with or without an offset (`T` or space separated).
pub fn parse_capture(value: &str) -> Option<CaptureTime> {
    let value = value.trim().replace(' ', "T");
    if let Ok(time) = DateTime::parse_from_rfc3339(&value) {
        return Some(CaptureTime::Absolute(time.timestamp_millis()));
    }
    NaiveDateTime::parse_from_str(&value, "%Y-%m-%dT%H:%M:%S%.f")
        .ok()
        .map(|t| CaptureTime::Local(t.and_utc().timestamp_millis()))
}

/// EXIF `±HH:MM` (OffsetTimeOriginal), in seconds east of UTC.
pub fn parse_utc_offset(value: &str) -> Option<i32> {
    let value = value.trim().trim_matches('\0').trim();
    if value.eq_ignore_ascii_case("z") {
        return Some(0);
    }
    let value = value
        .strip_prefix("UTC")
        .or_else(|| value.strip_prefix("GMT"))
        .unwrap_or(value)
        .trim();
    let (sign, rest) = match value.as_bytes().first()? {
        b'+' => (1, &value[1..]),
        b'-' | 0xE2 => (-1, value.trim_start_matches(['-', '−'])),
        _ => return None,
    };
    let (h, m) = rest.split_once(':').unwrap_or((rest, "0"));
    let (h, m): (i32, i32) = (h.trim().parse().ok()?, m.trim().parse().ok()?);
    if !(0..=14).contains(&h) || !(0..60).contains(&m) || (h == 14 && m != 0) {
        return None;
    }
    Some(sign * (h * 3600 + m * 60))
}

/// This computer's UTC offset on a given local date (so summer time is
/// right for the trip, not for today): the default photo time zone.
pub fn local_offset_at(local_millis: i64) -> Option<i32> {
    use chrono::{Local, Offset as _, TimeZone as _};
    let naive = DateTime::from_timestamp_millis(local_millis)?.naive_utc();
    Local
        .offset_from_local_datetime(&naive)
        .earliest()
        .map(|o| o.fix().local_minus_utc())
}

/// `+02:00` / `-05:30`, the form [`parse_utc_offset`] reads back.
pub fn format_utc_offset(seconds: i32) -> String {
    let sign = if seconds < 0 { '-' } else { '+' };
    let a = seconds.unsigned_abs();
    format!("{sign}{:02}:{:02}", a / 3600, (a / 60) % 60)
}

/// The capture time Schist would match: an XMP `exif:DateTimeOriginal`
/// (which may carry its own offset) wins; an explicitly empty one means
/// "no time". Otherwise EXIF DateTimeOriginal, made absolute by
/// OffsetTimeOriginal when the camera recorded one (else DateTime and
/// OffsetTime), with SubSecTimeOriginal for burst ordering.
pub fn capture_time(photo: &Path) -> Option<CaptureTime> {
    let capture = crate::variants::capture(photo);
    if let Ok(xmp) = crate::xmp::read(&capture) {
        if let Some(taken) = xmp.taken {
            return taken.as_deref().and_then(parse_capture);
        }
    }
    let data = crate::meta::exif_of(&capture)?;
    exif_capture_time(&data)
}

pub fn exif_capture_time(data: &exif::Exif) -> Option<CaptureTime> {
    let text = |tag| {
        data.get_field(tag, exif::In::PRIMARY)
            .and_then(|f| match &f.value {
                exif::Value::Ascii(parts) => parts
                    .first()
                    .map(|p| String::from_utf8_lossy(p).trim().to_string()),
                _ => None,
            })
    };
    let (time, offset, subsec) = match text(exif::Tag::DateTimeOriginal) {
        Some(time) => (
            time,
            text(exif::Tag::OffsetTimeOriginal),
            text(exif::Tag::SubSecTimeOriginal),
        ),
        None => (
            text(exif::Tag::DateTime)?,
            text(exif::Tag::OffsetTime),
            text(exif::Tag::SubSecTime),
        ),
    };
    // "2026:08:14 17:03:22"
    let local = NaiveDateTime::parse_from_str(&time, "%Y:%m:%d %H:%M:%S").ok()?;
    let mut millis = local.and_utc().timestamp_millis();
    if let Some(sub) = subsec.filter(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())) {
        let digits: String = sub.chars().chain("000".chars()).take(3).collect();
        millis += digits.parse::<i64>().unwrap_or(0);
    }
    Some(match offset.as_deref().and_then(parse_utc_offset) {
        Some(offset) => CaptureTime::Absolute(millis - i64::from(offset) * 1000),
        None => CaptureTime::Local(millis),
    })
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MatchOptions {
    /// The longest interval between two logged points, or between a photo and
    /// the nearest track end, that is still trusted.
    pub max_gap_seconds: i64,
    /// Added to every capture time: how far the camera clock was behind.
    pub clock_offset_seconds: i64,
    /// Seconds east of UTC for local capture times without an offset.
    pub local_utc_offset_seconds: i32,
}

impl Default for MatchOptions {
    fn default() -> Self {
        Self {
            max_gap_seconds: 300,
            clock_offset_seconds: 0,
            local_utc_offset_seconds: 0,
        }
    }
}

impl MatchOptions {
    pub fn utc_millis(&self, capture: CaptureTime) -> i64 {
        let base = match capture {
            CaptureTime::Absolute(t) => t,
            CaptureTime::Local(t) => t - i64::from(self.local_utc_offset_seconds) * 1000,
        };
        base.saturating_add(self.clock_offset_seconds.saturating_mul(1000))
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Match {
    /// A position, the elevation when both neighbours had one, and how far
    /// (seconds) the capture was from the nearest logged point.
    Found {
        lat: f64,
        lon: f64,
        ele: Option<f64>,
        nearest_seconds: f64,
    },
    /// Within the track's span, but in a gap longer than allowed.
    Gap,
    /// Before or after every segment by more than the allowed gap.
    Outside,
}

fn lerp_lon(a: f64, b: f64, f: f64) -> f64 {
    // Interpolate along the short way round, across the antimeridian if needed.
    let mut delta = b - a;
    if delta > 180.0 {
        delta -= 360.0;
    } else if delta < -180.0 {
        delta += 360.0;
    }
    let lon = a + delta * f;
    if lon > 180.0 {
        lon - 360.0
    } else if lon < -180.0 {
        lon + 360.0
    } else {
        lon
    }
}

/// Locate a UTC time on the tracks. Positions are linearly interpolated
/// between the two surrounding points of one segment when they are no more
/// than `max_gap_seconds` apart; a time just outside a segment snaps to its
/// end point under the same limit.
pub fn locate(tracks: &Tracks, utc_millis: i64, max_gap_seconds: i64) -> Match {
    let gap = max_gap_seconds.max(0).saturating_mul(1000);
    let mut best_end: Option<(i64, &TrackPoint)> = None;
    let mut inside_gap = false;
    for segment in &tracks.segments {
        let timed: Vec<(i64, &TrackPoint)> = segment.timed().collect();
        let (Some(&(first, _)), Some(&(last, _))) = (timed.first(), timed.last()) else {
            continue;
        };
        if (first..=last).contains(&utc_millis) {
            let at = timed.partition_point(|(t, _)| *t <= utc_millis);
            let (t0, p0) = timed[at - 1];
            if t0 == utc_millis {
                return Match::Found {
                    lat: p0.lat,
                    lon: p0.lon,
                    ele: p0.ele,
                    nearest_seconds: 0.0,
                };
            }
            let (t1, p1) = timed[at];
            if t1 - t0 > gap {
                inside_gap = true;
                continue;
            }
            let f = (utc_millis - t0) as f64 / (t1 - t0) as f64;
            return Match::Found {
                lat: p0.lat + (p1.lat - p0.lat) * f,
                lon: lerp_lon(p0.lon, p1.lon, f),
                ele: p0.ele.zip(p1.ele).map(|(a, b)| a + (b - a) * f),
                nearest_seconds: (utc_millis - t0).min(t1 - utc_millis) as f64 / 1000.0,
            };
        }
        for (t, p) in [timed[0], timed[timed.len() - 1]] {
            let distance = (t - utc_millis).abs();
            if distance <= gap && best_end.is_none_or(|(d, _)| distance < d) {
                best_end = Some((distance, p));
            }
        }
    }
    if let Some((distance, p)) = best_end {
        return Match::Found {
            lat: p.lat,
            lon: p.lon,
            ele: p.ele,
            nearest_seconds: distance as f64 / 1000.0,
        };
    }
    if inside_gap
        || tracks
            .span()
            .is_some_and(|(a, b)| (a..=b).contains(&utc_millis))
    {
        Match::Gap
    } else {
        Match::Outside
    }
}

/// What geotagging would do to one photo.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Plan {
    Write {
        lat: f64,
        lon: f64,
        ele: Option<f64>,
    },
    SkipHasGps,
    SkipNoTime,
    SkipGap,
    SkipOutside,
}

pub fn plan(
    tracks: &Tracks,
    capture: Option<CaptureTime>,
    has_gps: bool,
    skip_existing: bool,
    options: &MatchOptions,
) -> Plan {
    if has_gps && skip_existing {
        return Plan::SkipHasGps;
    }
    let Some(capture) = capture else {
        return Plan::SkipNoTime;
    };
    match locate(tracks, options.utc_millis(capture), options.max_gap_seconds) {
        Match::Found { lat, lon, ele, .. } => Plan::Write { lat, lon, ele },
        Match::Gap => Plan::SkipGap,
        Match::Outside => Plan::SkipOutside,
    }
}

/// Parse a signed duration typed by a person: seconds (`-90`), or
/// `[±][H:]MM:SS` (`+1:00:00`, `-0:30`).
pub fn parse_duration(value: &str) -> Option<i64> {
    let value = value.trim();
    if value.is_empty() {
        return Some(0);
    }
    let (sign, rest) = match value.as_bytes()[0] {
        b'-' => (-1, &value[1..]),
        b'+' => (1, &value[1..]),
        _ => (1, value),
    };
    let parts: Vec<&str> = rest.split(':').collect();
    if parts.len() > 3 || parts.iter().any(|p| p.is_empty()) {
        return None;
    }
    let mut total: i64 = 0;
    for (i, part) in parts.iter().enumerate() {
        let v: i64 = part.trim().parse().ok().filter(|v: &i64| *v >= 0)?;
        if i > 0 && v >= 60 {
            return None;
        }
        total = total.checked_mul(60)?.checked_add(v)?;
    }
    // Ten years either way is far beyond any clock error.
    (total <= 315_576_000).then_some(sign * total)
}

#[cfg(test)]
mod tests {
    use super::*;

    const GPX11: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<gpx version="1.1" creator="test" xmlns="http://www.topografix.com/GPX/1/1">
  <wpt lat="1" lon="1"><time>2024-05-01T09:00:00Z</time></wpt>
  <trk><name>Walk</name>
    <trkseg>
      <trkpt lat="51.5000" lon="-0.1000"><ele>10</ele><time>2024-05-01T10:00:00Z</time></trkpt>
      <trkpt lat="51.5100" lon="-0.1100"><ele>20</ele><time>2024-05-01T10:01:40Z</time></trkpt>
      <trkpt lat="51.5200" lon="-0.1200"><time>2024-05-01T10:03:20.500Z</time></trkpt>
    </trkseg>
    <trkseg>
      <trkpt lat="52.0000" lon="0.0000"><ele>5</ele><time>2024-05-01T12:00:00Z</time></trkpt>
      <trkpt lat="52.1000" lon="0.1000"><ele>15</ele><time>2024-05-01T12:30:00Z</time></trkpt>
    </trkseg>
  </trk>
</gpx>"#;

    const GPX10: &str = r#"<gpx version="1.0" xmlns="http://www.topografix.com/GPX/1/0">
  <time>2024-05-01T00:00:00Z</time>
  <trk><trkseg>
    <trkpt lat="40.0" lon="-74.0"><time>2024-05-01T15:00:10+02:00</time><ele>3.5</ele></trkpt>
    <trkpt lat="40.1" lon="-74.0"><time>2024-05-01T13:00:00Z</time></trkpt>
  </trkseg></trk>
</gpx>"#;

    fn ms(text: &str) -> i64 {
        parse_gpx_time(text).unwrap()
    }

    #[test]
    fn gpx_1_1_keeps_segments_elevation_and_fractional_times() {
        let tracks = parse(GPX11).unwrap();
        assert_eq!(tracks.segments.len(), 2);
        assert!(tracks.segments.iter().all(|s| s.source == Source::Track));
        let first = &tracks.segments[0].points;
        assert_eq!(first.len(), 3);
        assert_eq!(first[0].ele, Some(10.0));
        assert_eq!(first[2].ele, None);
        assert_eq!(first[2].time, Some(ms("2024-05-01T10:03:20.5Z")));
        assert_eq!(tracks.timed_points(), 5);
        assert_eq!(
            tracks.span(),
            Some((ms("2024-05-01T10:00:00Z"), ms("2024-05-01T12:30:00Z")))
        );
    }

    #[test]
    fn gpx_1_0_offsets_are_normalised_and_points_sorted() {
        let tracks = parse(GPX10).unwrap();
        let points = &tracks.segments[0].points;
        // 15:00:10+02:00 is 13:00:10Z, after the 13:00:00Z point.
        assert_eq!(points[0].time, Some(ms("2024-05-01T13:00:00Z")));
        assert_eq!(points[1].time, Some(ms("2024-05-01T13:00:10Z")));
        assert_eq!(points[1].ele, Some(3.5));
    }

    #[test]
    fn routes_then_waypoints_are_fallbacks_for_untimed_tracks() {
        let route = r#"<gpx xmlns="http://www.topografix.com/GPX/1/1">
            <trk><trkseg><trkpt lat="1" lon="1"/></trkseg></trk>
            <rte><rtept lat="2" lon="2"><time>2024-01-01T00:00:00Z</time></rtept>
                 <rtept lat="3" lon="3"><time>2024-01-01T00:01:00Z</time></rtept></rte>
        </gpx>"#;
        let tracks = parse(route).unwrap();
        assert_eq!(tracks.segments.len(), 1);
        assert_eq!(tracks.segments[0].source, Source::Route);
        let waypoints = r#"<gpx><wpt lat="5" lon="6"><time>2024-01-01T00:00:00Z</time></wpt>
            <wpt lat="7" lon="8"/></gpx>"#;
        let tracks = parse(waypoints).unwrap();
        assert_eq!(tracks.segments[0].source, Source::Waypoints);
        assert_eq!(tracks.segments[0].points.len(), 1);
        // Nothing timed at all: drawable, but never matched.
        let untimed =
            parse(r#"<gpx><trk><trkseg><trkpt lat="1" lon="2"/></trkseg></trk></gpx>"#).unwrap();
        assert_eq!(untimed.timed_points(), 0);
        assert_eq!(untimed.segments[0].points.len(), 1);
        assert_eq!(locate(&untimed, 0, 3600), Match::Outside);
    }

    #[test]
    fn malformed_gpx_is_refused_and_bad_points_are_counted() {
        assert!(parse("<gpx><trk>").is_err());
        assert!(parse("<kml></kml>").is_err());
        assert!(parse("not xml").is_err());
        assert!(parse(r#"<!DOCTYPE gpx [<!ENTITY x "y">]><gpx>&x;</gpx>"#).is_err());
        let bad = r#"<gpx><trk><trkseg>
            <trkpt lat="91" lon="0"><time>2024-01-01T00:00:00Z</time></trkpt>
            <trkpt lat="x" lon="0"><time>2024-01-01T00:00:00Z</time></trkpt>
            <trkpt lat="1" lon="1"><time>yesterday</time></trkpt>
            <trkpt lat="1" lon="1"><time>2024-01-01T00:00:00Z</time></trkpt>
            <trkpt lat="1" lon="1"><time>2024-01-01T00:00:00Z</time></trkpt>
        </trkseg></trk></gpx>"#;
        let tracks = parse(bad).unwrap();
        assert_eq!(tracks.skipped_points, 3);
        // The duplicate timestamp collapses to one point.
        assert_eq!(tracks.segments[0].points.len(), 1);
    }

    #[test]
    fn multiple_files_merge_in_time_order() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (dir.path().join("a.gpx"), dir.path().join("b.gpx"));
        std::fs::write(&a, GPX11).unwrap();
        std::fs::write(&b, GPX10).unwrap();
        let tracks = read_files(&[&a, &b]).unwrap();
        assert_eq!(tracks.segments.len(), 3);
        let starts: Vec<_> = tracks
            .segments
            .iter()
            .map(|s| s.span().unwrap().0)
            .collect();
        assert!(starts.windows(2).all(|w| w[0] <= w[1]));
        std::fs::write(&b, "<gpx><trk>").unwrap();
        assert!(read_files(&[&a, &b]).is_err());
    }

    #[test]
    fn interpolation_is_linear_within_a_segment() {
        let tracks = parse(GPX11).unwrap();
        let Match::Found {
            lat,
            lon,
            ele,
            nearest_seconds,
        } = locate(&tracks, ms("2024-05-01T10:00:25Z"), 300)
        else {
            panic!("expected a position");
        };
        assert!((lat - 51.5025).abs() < 1e-9);
        assert!((lon + 0.1025).abs() < 1e-9);
        assert!((ele.unwrap() - 12.5).abs() < 1e-9);
        assert_eq!(nearest_seconds, 25.0);
        // Exact hit; missing elevation on a neighbour gives none.
        assert!(matches!(
            locate(&tracks, ms("2024-05-01T10:01:40Z"), 300),
            Match::Found { lat, ele: Some(e), .. } if lat == 51.51 && e == 20.0
        ));
        assert!(matches!(
            locate(&tracks, ms("2024-05-01T10:02:00Z"), 300),
            Match::Found { ele: None, .. }
        ));
    }

    #[test]
    fn gaps_and_ends_respect_the_maximum() {
        let tracks = parse(GPX11).unwrap();
        // Between segments (10:03 .. 12:00): never interpolated across.
        assert_eq!(locate(&tracks, ms("2024-05-01T11:00:00Z"), 300), Match::Gap);
        // Within the second segment, whose points are 30 min apart.
        assert_eq!(locate(&tracks, ms("2024-05-01T12:10:00Z"), 300), Match::Gap);
        assert!(matches!(
            locate(&tracks, ms("2024-05-01T12:10:00Z"), 1800),
            Match::Found { .. }
        ));
        // Just after a segment's end snaps to it; further does not.
        assert!(matches!(
            locate(&tracks, ms("2024-05-01T10:05:00Z"), 300),
            Match::Found { lat, .. } if lat == 51.52
        ));
        assert!(matches!(
            locate(&tracks, ms("2024-05-01T11:58:00Z"), 300),
            Match::Found { lat, .. } if lat == 52.0
        ));
        assert_eq!(
            locate(&tracks, ms("2024-05-01T09:00:00Z"), 300),
            Match::Outside
        );
        assert_eq!(
            locate(&tracks, ms("2024-05-02T09:00:00Z"), 300),
            Match::Outside
        );
    }

    #[test]
    fn interpolation_crosses_the_antimeridian_the_short_way() {
        let gpx = r#"<gpx><trk><trkseg>
            <trkpt lat="0" lon="179"><time>2024-01-01T00:00:00Z</time></trkpt>
            <trkpt lat="0" lon="-179"><time>2024-01-01T00:00:20Z</time></trkpt>
        </trkseg></trk></gpx>"#;
        let tracks = parse(gpx).unwrap();
        let Match::Found { lon, .. } = locate(&tracks, ms("2024-01-01T00:00:15Z"), 60) else {
            panic!();
        };
        assert!((lon + 179.5).abs() < 1e-9, "{lon}");
    }

    #[test]
    fn time_zones_offsets_and_clock_corrections() {
        assert_eq!(parse_utc_offset("+02:00"), Some(7200));
        assert_eq!(parse_utc_offset("-05:30"), Some(-19800));
        assert_eq!(parse_utc_offset("UTC+1"), Some(3600));
        assert_eq!(parse_utc_offset("Z"), Some(0));
        assert_eq!(parse_utc_offset("+15:00"), None);
        assert_eq!(parse_utc_offset("02:00"), None);
        assert_eq!(parse_utc_offset("   :  "), None);
        for offset in [0, 7200, -19800, 45900, -43200] {
            assert_eq!(parse_utc_offset(&format_utc_offset(offset)), Some(offset));
        }

        let local = parse_capture("2024-05-01 12:00:25").unwrap();
        assert!(matches!(local, CaptureTime::Local(_)));
        let absolute = parse_capture("2024-05-01T12:00:25+02:00").unwrap();
        assert_eq!(absolute, CaptureTime::Absolute(ms("2024-05-01T10:00:25Z")));

        let tracks = parse(GPX11).unwrap();
        let options = MatchOptions {
            local_utc_offset_seconds: 7200,
            ..Default::default()
        };
        // Local 12:00:25 at UTC+2 and an explicit +02:00 land on the same point.
        assert_eq!(options.utc_millis(local), ms("2024-05-01T10:00:25Z"));
        assert_eq!(options.utc_millis(absolute), ms("2024-05-01T10:00:25Z"));
        // The zone setting never re-interprets an absolute time.
        let other_zone = MatchOptions {
            local_utc_offset_seconds: -3600,
            ..options
        };
        assert_eq!(other_zone.utc_millis(absolute), ms("2024-05-01T10:00:25Z"));
        // A camera 90 s slow: correction applies to both kinds.
        let slow = MatchOptions {
            clock_offset_seconds: 90,
            ..options
        };
        assert_eq!(slow.utc_millis(local), ms("2024-05-01T10:01:55Z"));
        assert_eq!(slow.utc_millis(absolute), ms("2024-05-01T10:01:55Z"));
        assert!(matches!(
            plan(&tracks, Some(local), false, true, &options),
            Plan::Write { .. }
        ));
        assert_eq!(
            plan(&tracks, Some(local), true, true, &options),
            Plan::SkipHasGps
        );
        assert!(matches!(
            plan(&tracks, Some(local), true, false, &options),
            Plan::Write { .. }
        ));
        assert_eq!(plan(&tracks, None, false, true, &options), Plan::SkipNoTime);
        // Read as UTC, the same local time is two hours after the track.
        assert_eq!(
            plan(&tracks, Some(local), false, true, &MatchOptions::default()),
            Plan::SkipGap
        );
    }

    #[test]
    fn exif_offset_time_original_makes_a_capture_absolute() {
        use exif::experimental::Writer;
        use exif::{Field, In, Tag, Value};
        let ascii = |tag, s: &str| Field {
            tag,
            ifd_num: In::PRIMARY,
            value: Value::Ascii(vec![s.as_bytes().to_vec()]),
        };
        let build = |fields: &[Field]| {
            let mut writer = Writer::new();
            for field in fields {
                writer.push_field(field);
            }
            let mut out = std::io::Cursor::new(Vec::new());
            writer.write(&mut out, false).unwrap();
            exif::Reader::new().read_raw(out.into_inner()).unwrap()
        };
        let local = build(&[
            ascii(Tag::DateTimeOriginal, "2024:05:01 12:00:25"),
            ascii(Tag::SubSecTimeOriginal, "5"),
        ]);
        assert_eq!(
            exif_capture_time(&local),
            Some(CaptureTime::Local(ms("2024-05-01T12:00:25.5Z")))
        );
        let zoned = build(&[
            ascii(Tag::DateTimeOriginal, "2024:05:01 12:00:25"),
            ascii(Tag::OffsetTimeOriginal, "+02:00"),
        ]);
        assert_eq!(
            exif_capture_time(&zoned),
            Some(CaptureTime::Absolute(ms("2024-05-01T10:00:25Z")))
        );
    }

    #[test]
    fn typed_durations() {
        assert_eq!(parse_duration(""), Some(0));
        assert_eq!(parse_duration("-90"), Some(-90));
        assert_eq!(parse_duration("+1:00:00"), Some(3600));
        assert_eq!(parse_duration("-0:30"), Some(-30));
        assert_eq!(parse_duration("1:75"), None);
        assert_eq!(parse_duration("abc"), None);
        assert_eq!(parse_duration("1::2"), None);
    }
}
