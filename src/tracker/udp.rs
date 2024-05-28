use std::io::Cursor;

use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[derive(Debug, Clone)]
pub struct UDPConnectRequest {
    pub transaction_id: u32,
}

impl UDPConnectRequest {
    const PROTOCOL_ID: u64 = 0x41727101980;
    const ACTION: u32 = 0;

    pub fn new(transaction_id: u32) -> Self {
        Self { transaction_id }
    }

    pub async fn to_bytes(&self) -> anyhow::Result<[u8; 16]> {
        let mut bytes = [0; 16];
        let mut cursor = Cursor::new(&mut bytes[..]);

        cursor.write_u64(Self::PROTOCOL_ID).await?;
        cursor.write_u32(Self::ACTION).await?;
        cursor.write_u32(self.transaction_id).await?;

        Ok(bytes)
    }
}

#[derive(Debug, Clone)]
pub struct UDPConnectResponse {
    pub action: u32,
    pub transaction_id: u32,
    pub connection_id: u64,
}

impl UDPConnectResponse {
    pub async fn from_bytes(response: Vec<u8>) -> anyhow::Result<Self> {
        let mut cursor = Cursor::new(response);
        let action = cursor.read_u32().await?;
        let transaction_id = cursor.read_u32().await?;
        let connection_id = cursor.read_u64().await?;
        Ok(Self {
            action,
            transaction_id,
            connection_id,
        })
    }
}
