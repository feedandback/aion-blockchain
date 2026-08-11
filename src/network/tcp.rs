use tokio::io::{
    AsyncReadExt,
    AsyncWriteExt,
};
use tokio::net::{
    TcpListener,
    TcpStream,
};
use tokio::sync::mpsc;

use crate::network::NetworkMessage;
use crate::protocol::MAX_NETWORK_MESSAGE_BYTES;

pub struct TcpTransport;

#[allow(dead_code)]
impl TcpTransport {
    pub async fn bind(
        address: &str,
    ) -> Result<TcpListener, String> {
        TcpListener::bind(
            address,
        )
        .await
        .map_err(
            |error| {
                format!(
                    "TCP listener başlatılamadı: {}",
                    error
                )
            },
        )
    }

    pub async fn connect(
        address: &str,
    ) -> Result<TcpStream, String> {
        TcpStream::connect(
            address,
        )
        .await
        .map_err(
            |error| {
                format!(
                    "Peer bağlantısı kurulamadı: {}",
                    error
                )
            },
        )
    }

    pub async fn send_message(
        stream: &mut TcpStream,
        message: &NetworkMessage,
    ) -> Result<(), String> {
        let payload =
            serde_json::to_vec(
                message,
            )
            .map_err(
                |error| {
                    format!(
                        "Network mesajı JSON'a çevrilemedi: {}",
                        error
                    )
                },
            )?;

        if payload.is_empty() {
            return Err(
                "Boş network mesajı gönderilemez"
                    .into(),
            );
        }

        if payload.len()
            > MAX_NETWORK_MESSAGE_BYTES
        {
            return Err(
                "Network mesajı byte limiti aşıldı"
                    .into(),
            );
        }

        let payload_length =
            u32::try_from(
                payload.len(),
            )
            .map_err(
                |_| {
                    "Network mesaj uzunluğu u32 sınırını aşıyor"
                        .to_string()
                },
            )?;

        stream
            .write_all(
                &payload_length
                    .to_be_bytes(),
            )
            .await
            .map_err(
                |error| {
                    format!(
                        "Network mesaj başlığı gönderilemedi: {}",
                        error
                    )
                },
            )?;

        stream
            .write_all(
                &payload,
            )
            .await
            .map_err(
                |error| {
                    format!(
                        "Network mesajı gönderilemedi: {}",
                        error
                    )
                },
            )?;

        stream
            .flush()
            .await
            .map_err(
                |error| {
                    format!(
                        "Network mesajı flush edilemedi: {}",
                        error
                    )
                },
            )?;

        Ok(())
    }

    pub async fn read_message(
        stream: &mut TcpStream,
    ) -> Result<NetworkMessage, String> {
        let mut length_bytes =
            [0u8; 4];

        stream
            .read_exact(
                &mut length_bytes,
            )
            .await
            .map_err(
                |error| {
                    format!(
                        "Network mesaj başlığı okunamadı: {}",
                        error
                    )
                },
            )?;

        let payload_length =
            u32::from_be_bytes(
                length_bytes,
            ) as usize;

        if payload_length == 0 {
            return Err(
                "Boş network mesajı reddedildi"
                    .into(),
            );
        }

        if payload_length
            > MAX_NETWORK_MESSAGE_BYTES
        {
            return Err(
                "Network mesajı byte limiti aşıldı"
                    .into(),
            );
        }

        let mut payload =
            vec![
                0u8;
                payload_length
            ];

        stream
            .read_exact(
                &mut payload,
            )
            .await
            .map_err(
                |error| {
                    format!(
                        "Network mesajı okunamadı: {}",
                        error
                    )
                },
            )?;

        serde_json::from_slice(
            &payload,
        )
        .map_err(
            |error| {
                format!(
                    "Network mesajı JSON formatı geçersiz: {}",
                    error
                )
            },
        )
    }

    pub async fn run_listener(
        address: &str,
        sender: mpsc::Sender<(
            NetworkMessage,
            String,
        )>,
    ) -> Result<(), String> {
        let listener =
            Self::bind(
                address,
            )
            .await?;

        println!(
            "🌐 AION TCP listener aktif: {}",
            address
        );

        loop {
            let (
                mut stream,
                peer_address,
            ) =
                listener
                    .accept()
                    .await
                    .map_err(
                        |error| {
                            format!(
                                "TCP peer kabul edilemedi: {}",
                                error
                            )
                        },
                    )?;

            let message =
                match Self::read_message(
                    &mut stream,
                )
                .await
                {
                    Ok(message) => {
                        message
                    }

                    Err(error) => {
                        println!(
                            "❌ TCP mesajı reddedildi: {}",
                            error
                        );

                        continue;
                    }
                };

            if sender
                .send((
                    message,
                    peer_address
                        .to_string(),
                ))
                .await
                .is_err()
            {
                return Err(
                    "TCP mesaj kanalı kapandı"
                        .into(),
                );
            }
        }
    }

    pub async fn send_to(
        address: &str,
        message: &NetworkMessage,
    ) -> Result<(), String> {
        let mut stream =
            Self::connect(
                address,
            )
            .await?;

        Self::send_message(
            &mut stream,
            message,
        )
        .await
    }

    pub async fn accept_one(
        listener: &TcpListener,
    ) -> Result<
        (
            NetworkMessage,
            String,
        ),
        String,
    > {
        let (
            mut stream,
            peer_address,
        ) =
            listener
                .accept()
                .await
                .map_err(
                    |error| {
                        format!(
                            "TCP peer kabul edilemedi: {}",
                            error
                        )
                    },
                )?;

        let message =
            Self::read_message(
                &mut stream,
            )
            .await?;

        Ok((
            message,
            peer_address
                .to_string(),
        ))
    }
}