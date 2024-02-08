use serde::de::{self, Visitor};
use serde::Deserialize;
use std::net::{Ipv4Addr, SocketAddrV4};

#[derive(Debug, Clone)]
pub struct PeerAddresses(Vec<SocketAddrV4>);

impl<'de> Deserialize<'de> for PeerAddresses {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Ok(PeerAddresses(
            deserializer.deserialize_bytes(SocketAddressesVisitor)?,
        ))
    }
}

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
                let [addr1, addr2, addr3, addr4, port @ ..]: [u8; Self::SOCKET_ADDR_SIZE_BYTES] =
                    socket_addr_bytes
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
