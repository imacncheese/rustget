use serde::{Deserialize, Serialize};
use thiserror::Error;
use url::Url;

#[derive(Debug, Error)]
pub enum DashError {
    #[error("invalid MPD XML: {0}")]
    Xml(#[from] roxmltree::Error),
    #[error("invalid base URL: {0}")]
    Url(#[from] url::ParseError),
    #[error("MPD has no representations")]
    Empty,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashRepresentation {
    pub id: String,
    pub bandwidth: u64,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub mime_type: Option<String>,
    pub codecs: Option<String>,
    pub base_url: String,
    pub initialization: Option<String>,
    pub media_template: Option<String>,
    pub start_number: u64,
    pub duration_per_segment: Option<u64>,
    pub timescale: u64,
    pub period_duration_secs: Option<f64>,
    pub drm: bool,
}

impl DashRepresentation {
    /// Number of media segments for a simple duration-based SegmentTemplate.
    /// SegmentTimeline, multi-period and live manifests return `None`.
    #[must_use]
    pub fn segment_count(&self) -> Option<u64> {
        let duration = self.duration_per_segment?;
        let total = self.period_duration_secs?;
        if duration == 0 || self.timescale == 0 {
            return None;
        }
        let count = (total * self.timescale as f64 / duration as f64).ceil();
        (count > 0.0).then_some(count as u64)
    }

    #[must_use]
    pub fn segment_url(&self, number: u64) -> Option<String> {
        let template = self.media_template.as_ref()?;
        let time =
            number.saturating_sub(self.start_number) * self.duration_per_segment.unwrap_or(1);
        Some(
            expand_template(template, &self.id, self.bandwidth, number)
                .replace("$Time$", &time.to_string()),
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashManifest {
    pub duration_secs: Option<f64>,
    pub representations: Vec<DashRepresentation>,
}

/// Parse a DASH MPD and expose the representations and SegmentTemplate fields
/// needed for a first-segment probe. Full timeline construction is deliberately
/// left to the future DASH scheduler.
pub fn parse_mpd(text: &str, base: &Url) -> Result<DashManifest, DashError> {
    let document = roxmltree::Document::parse(text)?;
    let mpd = document.root_element();
    let duration_secs = mpd
        .attribute("mediaPresentationDuration")
        .and_then(parse_duration);
    let mpd_base = mpd
        .children()
        .find(|node| node.has_tag_name("BaseURL"))
        .and_then(|node| node.text())
        .map(|value| base.join(value.trim()))
        .transpose()?
        .unwrap_or_else(|| base.clone());
    let mut representations = Vec::new();
    for period in mpd.descendants().filter(|node| node.has_tag_name("Period")) {
        let period_base = period
            .children()
            .find(|node| node.has_tag_name("BaseURL"))
            .and_then(|node| node.text())
            .map(|value| mpd_base.join(value.trim()))
            .transpose()?
            .unwrap_or_else(|| mpd_base.clone());
        for adaptation in period
            .children()
            .filter(|node| node.has_tag_name("AdaptationSet"))
        {
            let adaptation_base = adaptation
                .children()
                .find(|node| node.has_tag_name("BaseURL"))
                .and_then(|node| node.text())
                .map(|value| period_base.join(value.trim()))
                .transpose()?
                .unwrap_or_else(|| period_base.clone());
            for representation in adaptation
                .children()
                .filter(|node| node.has_tag_name("Representation"))
            {
                let rep_base = representation
                    .children()
                    .find(|node| node.has_tag_name("BaseURL"))
                    .and_then(|node| node.text())
                    .map(|value| adaptation_base.join(value.trim()))
                    .transpose()?
                    .unwrap_or_else(|| adaptation_base.clone());
                let template = representation
                    .children()
                    .find(|node| node.has_tag_name("SegmentTemplate"))
                    .or_else(|| {
                        adaptation
                            .children()
                            .find(|node| node.has_tag_name("SegmentTemplate"))
                    });
                let id = representation
                    .attribute("id")
                    .unwrap_or("default")
                    .to_string();
                let bandwidth = representation
                    .attribute("bandwidth")
                    .and_then(|value| value.parse().ok())
                    .unwrap_or(0);
                let start_number = template
                    .and_then(|node| node.attribute("startNumber"))
                    .and_then(|value| value.parse().ok())
                    .unwrap_or(1);
                let duration_per_segment = template
                    .and_then(|node| node.attribute("duration"))
                    .and_then(|value| value.parse().ok());
                let timescale = template
                    .and_then(|node| node.attribute("timescale"))
                    .and_then(|value| value.parse().ok())
                    .unwrap_or(1);
                let period_duration_secs = period
                    .attribute("duration")
                    .and_then(parse_duration)
                    .or(duration_secs);
                let drm = adaptation
                    .children()
                    .any(|node| node.has_tag_name("ContentProtection"))
                    || representation
                        .children()
                        .any(|node| node.has_tag_name("ContentProtection"));
                representations.push(DashRepresentation {
                    id: id.clone(),
                    bandwidth,
                    width: representation
                        .attribute("width")
                        .and_then(|value| value.parse().ok()),
                    height: representation
                        .attribute("height")
                        .and_then(|value| value.parse().ok()),
                    mime_type: representation
                        .attribute("mimeType")
                        .or_else(|| adaptation.attribute("mimeType"))
                        .map(str::to_string),
                    codecs: representation
                        .attribute("codecs")
                        .or_else(|| adaptation.attribute("codecs"))
                        .map(str::to_string),
                    base_url: rep_base.to_string(),
                    initialization: template
                        .and_then(|node| node.attribute("initialization"))
                        .map(|value| {
                            let value = expand_template(value, &id, bandwidth, start_number);
                            rep_base.join(&value).map(|url| url.to_string())
                        })
                        .transpose()?,
                    media_template: template
                        .and_then(|node| node.attribute("media"))
                        .map(str::to_string),
                    start_number,
                    duration_per_segment,
                    timescale,
                    period_duration_secs,
                    drm,
                });
            }
        }
    }
    if representations.is_empty() {
        return Err(DashError::Empty);
    }
    representations.sort_by_key(|representation| representation.bandwidth);
    Ok(DashManifest {
        duration_secs,
        representations,
    })
}

fn expand_template(value: &str, id: &str, bandwidth: u64, number: u64) -> String {
    value
        .replace("$RepresentationID$", id)
        .replace("$Bandwidth$", &bandwidth.to_string())
        .replace("$Number$", &number.to_string())
        .replace("$Number%05d$", &format!("{number:05}"))
}

fn parse_duration(value: &str) -> Option<f64> {
    // ISO-8601 duration subset: PT#H#M#S.
    let value = value.strip_prefix("PT")?;
    let mut seconds = 0.0;
    let mut number = String::new();
    for ch in value.chars() {
        match ch {
            '0'..='9' | '.' => number.push(ch),
            'H' => {
                seconds += number.parse::<f64>().ok()? * 3600.0;
                number.clear();
            }
            'M' => {
                seconds += number.parse::<f64>().ok()? * 60.0;
                number.clear();
            }
            'S' => {
                seconds += number.parse::<f64>().ok()?;
                number.clear();
            }
            _ => return None,
        }
    }
    Some(seconds)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_representations_and_templates() {
        let mpd = r#"<?xml version="1.0"?>
<MPD xmlns="urn:mpeg:dash:schema:mpd:2011" mediaPresentationDuration="PT1M30S">
  <Period>
    <AdaptationSet mimeType="video/mp4" codecs="avc1.4d401f">
      <BaseURL>video/</BaseURL>
      <Representation id="720" bandwidth="2500000" width="1280" height="720">
        <SegmentTemplate initialization="init-$RepresentationID$.mp4" media="seg-$Number$.m4s" startNumber="0"/>
      </Representation>
    </AdaptationSet>
  </Period>
</MPD>"#;
        let base = Url::parse("https://cdn.example.test/movie/manifest.mpd").unwrap();
        let parsed = parse_mpd(mpd, &base).unwrap();
        assert_eq!(parsed.duration_secs, Some(90.0));
        assert_eq!(parsed.representations.len(), 1);
        let representation = &parsed.representations[0];
        assert_eq!(representation.width, Some(1280));
        assert_eq!(representation.start_number, 0);
        assert_eq!(
            representation.initialization.as_deref(),
            Some("https://cdn.example.test/movie/video/init-720.mp4")
        );
        assert!(representation.media_template.is_some());
    }

    #[test]
    fn duration_template_segment_count_and_urls() {
        let mpd = r#"<MPD xmlns="urn:mpeg:dash:schema:mpd:2011" mediaPresentationDuration="PT6S">
  <Period>
    <AdaptationSet mimeType="video/mp4">
      <Representation id="v" bandwidth="1000">
        <SegmentTemplate duration="2" timescale="1" startNumber="1"
          initialization="init-$RepresentationID$.mp4" media="seg-$Number$.m4s"/>
      </Representation>
    </AdaptationSet>
  </Period>
</MPD>"#;
        let base = Url::parse("https://cdn.example.test/x/manifest.mpd").unwrap();
        let parsed = parse_mpd(mpd, &base).unwrap();
        let rep = &parsed.representations[0];
        assert_eq!(rep.segment_count(), Some(3));
        assert_eq!(rep.segment_url(1), Some("seg-1.m4s".into()));
        assert_eq!(rep.segment_url(3), Some("seg-3.m4s".into()));
    }
}
