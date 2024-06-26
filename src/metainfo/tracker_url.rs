use reqwest::IntoUrl;
use reqwest::Url;
use serde::{de::Visitor, Deserialize};

#[derive(Clone, Debug)]
pub struct UdpUrl(Url);
#[derive(Clone, Debug)]
pub struct HttpUrl(Url);

#[derive(Debug, Clone)]
pub enum TrackerUrl {
    Http(HttpUrl),
    Udp(UdpUrl),
}

impl AsRef<str> for HttpUrl {
    fn as_ref(&self) -> &str {
        self.0.as_ref()
    }
}

impl AsRef<str> for UdpUrl {
    fn as_ref(&self) -> &str {
        self.0.as_ref()
    }
}

impl HttpUrl {
    pub fn into_inner(self) -> Url {
        self.0
    }
}

impl UdpUrl {
    fn into_inner(self) -> Url {
        self.0
    }
}

impl Into<Url> for HttpUrl {
    fn into(self) -> Url {
        self.into_inner()
    }
}

impl Into<Url> for UdpUrl {
    fn into(self) -> Url {
        self.into_inner()
    }
}

impl TrackerUrl {
    fn new(url: impl IntoUrl) -> anyhow::Result<Self> {
        let url = url.into_url()?;
        Ok(match url.scheme() {
            "http" => Self::Http(HttpUrl(url)),
            "udp" => Self::Udp(UdpUrl(url)),
            scheme => anyhow::bail!(format!("unsupported scheme {:?} for tracker", scheme)),
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
