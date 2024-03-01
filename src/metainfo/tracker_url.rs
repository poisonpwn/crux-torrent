use reqwest::IntoUrl;
use serde::{de::Visitor, Deserialize};

#[derive(Debug, Clone)]
pub enum TrackerUrl {
    HTTP(String),
    UDP(String),
}

impl TrackerUrl {
    fn new(url: impl IntoUrl) -> anyhow::Result<Self> {
        let url = url.into_url()?;
        Ok(match url.scheme() {
            "http" => Self::HTTP(url.into()),
            "udp" => Self::UDP(url.into()),
            scheme @ _ => anyhow::bail!(format!("unsupported scheme {:?} for tracker", scheme)),
        })
    }
}

impl<'a> Deserialize<'a> for TrackerUrl {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'a>,
    {
        deserializer.deserialize_string(TrackerUrlVisitor)
    }
}

struct TrackerUrlVisitor;
impl<'a> Visitor<'a> for TrackerUrlVisitor {
    type Value = TrackerUrl;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("string url using udp or http scheme")
    }

    // this is what serde_bencode calls for deserializing str.
    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        self.visit_string(v.to_owned())
    }

    fn visit_string<E>(self, v: String) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        TrackerUrl::new(v).map_err(serde::de::Error::custom)
    }
}
