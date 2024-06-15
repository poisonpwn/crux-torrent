use serde::Deserialize;
use std::net::SocketAddrV4;

#[derive(Debug, Clone, Deserialize)]
pub struct TrackerResponse {
    #[serde(rename = "interval")]
    pub request_interval_seconds: u64,

    #[serde(rename = "peers")]
    #[serde(deserialize_with = "parsing::deserialize_socket_addrs")]
    pub peer_addreses: Vec<SocketAddrV4>,
}

// this struct is seperate so that it  can be deserialized properly and can be converted into a Result whose Ok variant gives the successful TrackerResponse.
#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub enum TrackerResponseResult {
    Success(TrackerResponse),
    Failure {
        #[serde(rename = "failure reason")]
        failure_reason: String,
    },
}

impl From<TrackerResponseResult> for anyhow::Result<TrackerResponse> {
    fn from(value: TrackerResponseResult) -> Self {
        type TR = TrackerResponseResult;
        match value {
            TR::Success(tracker_response) => Ok(tracker_response),
            TR::Failure { failure_reason } => {
                anyhow::bail!(format!("{} (Tracker)", failure_reason))
            }
        }
    }
}

mod parsing {
    use serde::de::{self, Deserializer, Visitor};
    use std::net::{Ipv4Addr, SocketAddrV4};

    struct SocketAddressesVisitor;
    impl SocketAddressesVisitor {
        const SOCKET_ADDR_SIZE_BYTES: usize = 6;
    }

    impl<'de> Visitor<'de> for SocketAddressesVisitor {
        type Value = Vec<SocketAddrV4>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str(
                "continuous byte string of encoded socket addresses, each 6 bytes long, where the first 4 bytes specify the ipv4 address, and next 2 specify the port." 
            )
        }

        fn visit_bytes<E>(self, bytes: &[u8]) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            // peers should be a list of byte chunks each 6 long with no remainder at the end.
            let addr_byte_chunks = bytes.chunks_exact(Self::SOCKET_ADDR_SIZE_BYTES);

            if !addr_byte_chunks.remainder().is_empty() {
                return Err(E::custom(
                    "socket addresses byte string should have a length which is a multiple of 6",
                ));
            }

            //TODO: use slice.array_chunks::<6> when it becomes stable.
            let socket_addresses = addr_byte_chunks
                .map(|socket_addr_bytes| {
                    let [addr1, addr2, addr3, addr4, port @ ..]: [u8;
                        Self::SOCKET_ADDR_SIZE_BYTES] = socket_addr_bytes
                        .try_into()
                        .expect("chunks exact returns slices of exactly length 6");

                    let ip_addr = Ipv4Addr::new(addr1, addr2, addr3, addr4);
                    let port = u16::from_be_bytes(port);

                    SocketAddrV4::new(ip_addr, port)
                })
                .collect();

            Ok(socket_addresses)
        }
    }

    pub fn deserialize_socket_addrs<'de, D>(deserializer: D) -> Result<Vec<SocketAddrV4>, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_bytes(SocketAddressesVisitor)
    }
}
