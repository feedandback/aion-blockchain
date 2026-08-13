use tokio::io::{
    AsyncReadExt,
    AsyncWriteExt,
};
use tokio::net::{
    TcpListener,
    TcpStream,
};
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{
    mpsc,
    Semaphore,
};
use tokio::time::timeout;

use crate::network::{
    Network,
    NetworkMessage,
};
use crate::wallet::Wallet;
use crate::protocol::{
    MAX_CONCURRENT_NETWORK_CONNECTIONS,
    MAX_NETWORK_MESSAGE_BYTES,
    NETWORK_CONNECT_TIMEOUT_SECONDS,
    NETWORK_IO_TIMEOUT_SECONDS,
};

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
        timeout(
            Duration::from_secs(
                NETWORK_CONNECT_TIMEOUT_SECONDS,
            ),
            TcpStream::connect(
                address,
            ),
        )
        .await
        .map_err(
            |_| {
                format!(
                    "Peer bağlantısı zaman aşımına uğradı: {} saniye",
                    NETWORK_CONNECT_TIMEOUT_SECONDS
                )
            },
        )?
        .map_err(
            |error| {
                format!(
                    "Peer bağlantısı kurulamadı: {}",
                    error
                )
            },
        )
    }

    pub async fn accept_connection(
        listener: &TcpListener,
    ) -> Result<(TcpStream, String), String> {
        let (
            stream,
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

        Ok((
            stream,
            peer_address.to_string(),
        ))
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

        timeout(
            Duration::from_secs(
                NETWORK_IO_TIMEOUT_SECONDS,
            ),
            stream.write_all(
                &payload_length
                    .to_be_bytes(),
            ),
        )
        .await
        .map_err(
            |_| {
                "Network mesaj başlığı gönderimi zaman aşımına uğradı"
                    .to_string()
            },
        )?
        .map_err(
            |error| {
                format!(
                    "Network mesaj başlığı gönderilemedi: {}",
                    error
                )
            },
        )?;

        timeout(
            Duration::from_secs(
                NETWORK_IO_TIMEOUT_SECONDS,
            ),
            stream.write_all(
                &payload,
            ),
        )
        .await
        .map_err(
            |_| {
                "Network mesajı gönderimi zaman aşımına uğradı"
                    .to_string()
            },
        )?
        .map_err(
            |error| {
                format!(
                    "Network mesajı gönderilemedi: {}",
                    error
                )
            },
        )?;

        timeout(
            Duration::from_secs(
                NETWORK_IO_TIMEOUT_SECONDS,
            ),
            stream.flush(),
        )
        .await
        .map_err(
            |_| {
                "Network mesaj flush işlemi zaman aşımına uğradı"
                    .to_string()
            },
        )?
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

        timeout(
            Duration::from_secs(
                NETWORK_IO_TIMEOUT_SECONDS,
            ),
            stream.read_exact(
                &mut length_bytes,
            ),
        )
        .await
        .map_err(
            |_| {
                "Network mesaj başlığı okuma zaman aşımına uğradı"
                    .to_string()
            },
        )?
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

        timeout(
            Duration::from_secs(
                NETWORK_IO_TIMEOUT_SECONDS,
            ),
            stream.read_exact(
                &mut payload,
            ),
        )
        .await
        .map_err(
            |_| {
                "Network mesajı okuma zaman aşımına uğradı"
                    .to_string()
            },
        )?
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

        let connection_limit =
            Arc::new(
                Semaphore::new(
                    MAX_CONCURRENT_NETWORK_CONNECTIONS,
                ),
            );

        println!(
            "🌐 Kybernetes TCP listener aktif: {}",
            address
        );

        println!(
            "🛡️ Maksimum eşzamanlı TCP bağlantısı: {}",
            MAX_CONCURRENT_NETWORK_CONNECTIONS
        );

        loop {
            if sender.is_closed() {
                return Err(
                    "TCP mesaj kanalı kapandı"
                        .into(),
                );
            }

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

            let permit =
                match connection_limit
                    .clone()
                    .try_acquire_owned()
                {
                    Ok(permit) => permit,

                    Err(_) => {
                        println!(
                            "❌ TCP bağlantısı reddedildi: Eşzamanlı bağlantı limiti ({}) dolu",
                            MAX_CONCURRENT_NETWORK_CONNECTIONS
                        );

                        continue;
                    }
                };

            let peer_address =
                peer_address.to_string();

            let message_sender =
                sender.clone();

            tokio::spawn(
                async move {
                    let _permit =
                        permit;

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

                                return;
                            }
                        };

                    if message_sender
                        .send((
                            message,
                            peer_address,
                        ))
                        .await
                        .is_err()
                    {
                        println!(
                            "❌ TCP mesajı aktarılamadı: Mesaj kanalı kapandı"
                        );
                    }
                },
            );
        }
    }

    pub async fn run_authenticated_listener(
        address: &str,
        wallet: Arc<Wallet>,
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

        let connection_limit =
            Arc::new(
                Semaphore::new(
                    MAX_CONCURRENT_NETWORK_CONNECTIONS,
                ),
            );

        println!(
            "🌐 Kimliği doğrulanmış Kybernetes P2P listener aktif: {}",
            address
        );

        loop {
            if sender.is_closed() {
                return Err(
                    "P2P mesaj kanalı kapandı"
                        .into(),
                );
            }

            let permit =
                connection_limit
                    .clone()
                    .acquire_owned()
                    .await
                    .map_err(
                        |_| {
                            "P2P bağlantı limiti kapandı"
                                .to_string()
                        },
                    )?;

            let (
                stream,
                peer_address,
            ) =
                Self::accept_connection(
                    &listener,
                )
                .await?;

            let peer_wallet =
                wallet.clone();

            let message_sender =
                sender.clone();

            tokio::spawn(
                async move {
                    let _permit =
                        permit;

                    let mut stream =
                        stream;

                    let handshake =
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
                                    "❌ P2P handshake okunamadı: {}",
                                    error
                                );

                                return;
                            }
                        };

                    let challenge =
                        Network::handshake_challenge(
                            &handshake,
                        )
                        .unwrap_or("")
                        .to_string();

                    let mut validation_network =
                        Network::new();

                    validation_network.receive(
                        handshake.clone(),
                    );

                    let accepted =
                        validation_network
                            .identified_peer_count()
                            == 1;

                    let handshake_ack =
                        Network::create_handshake_ack(
                            &peer_wallet,
                            accepted,
                            challenge,
                        );

                    if let Err(error) =
                        Self::send_message(
                            &mut stream,
                            &handshake_ack,
                        )
                        .await
                    {
                        println!(
                            "❌ HandshakeAck gönderilemedi: {}",
                            error
                        );

                        return;
                    }

                    if !accepted {
                        println!(
                            "❌ Kimliği doğrulanmamış peer reddedildi: {}",
                            peer_address
                        );

                        return;
                    }

                    println!(
                        "✅ Kimliği doğrulanmış peer bağlandı: {}",
                        peer_address
                    );

                    if message_sender
                        .send((
                            handshake,
                            peer_address
                                .clone(),
                        ))
                        .await
                        .is_err()
                    {
                        println!(
                            "❌ Peer kimliği ana node'a aktarılamadı"
                        );

                        return;
                    }

                    loop {
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
                                        "🔌 Peer bağlantısı kapandı: {} ({})",
                                        peer_address,
                                        error
                                    );

                                    break;
                                }
                            };

                        if message_sender
                            .send((
                                message,
                                peer_address
                                    .clone(),
                            ))
                            .await
                            .is_err()
                        {
                            println!(
                                "❌ P2P mesaj kanalı kapandı"
                            );

                            break;
                        }
                    }
                },
            );
        }
    }

    pub async fn broadcast_authenticated(
        peer_addresses: &[String],
        wallet: &Wallet,
        listen_address: &str,
        message: &NetworkMessage,
    ) -> (
        usize,
        usize,
    ) {
        let mut success_count =
            0usize;

        let mut failure_count =
            0usize;

        for peer_address in peer_addresses {
            match Self::send_authenticated_message(
                peer_address,
                wallet,
                listen_address,
                message,
            )
            .await
            {
                Ok(()) => {
                    success_count += 1;

                    println!(
                        "✅ P2P broadcast gönderildi: {}",
                        peer_address
                    );
                }

                Err(error) => {
                    failure_count += 1;

                    println!(
                        "❌ P2P broadcast başarısız: {} ({})",
                        peer_address,
                        error
                    );
                }
            }
        }

        (
            success_count,
            failure_count,
        )
    }

    pub async fn send_authenticated_message(
        address: &str,
        wallet: &Wallet,
        listen_address: &str,
        message: &NetworkMessage,
    ) -> Result<(), String> {
        let mut stream =
            Self::connect_authenticated(
                address,
                wallet,
                listen_address,
            )
            .await?;

        Self::send_message(
            &mut stream,
            message,
        )
        .await
    }

    pub async fn send_authenticated_request(
        address: &str,
        wallet: &Wallet,
        listen_address: &str,
        request: &NetworkMessage,
    ) -> Result<NetworkMessage, String> {
        let mut stream =
            Self::connect_authenticated(
                address,
                wallet,
                listen_address,
            )
            .await?;

        Self::send_message(
            &mut stream,
            request,
        )
        .await?;

        Self::read_message(
            &mut stream,
        )
        .await
    }

    pub async fn connect_authenticated(
        address: &str,
        wallet: &Wallet,
        listen_address: &str,
    ) -> Result<TcpStream, String> {
        let handshake =
            Network::create_handshake(
                wallet,
                listen_address
                    .to_string(),
            );

        let expected_challenge =
            Network::handshake_challenge(
                &handshake,
            )
            .ok_or_else(
                || {
                    "Handshake challenge oluşturulamadı"
                        .to_string()
                },
            )?
            .to_string();

        let mut stream =
            Self::connect(
                address,
            )
            .await?;

        Self::send_message(
            &mut stream,
            &handshake,
        )
        .await?;

        let response =
            Self::read_message(
                &mut stream,
            )
            .await?;

        if !Network::validate_handshake_ack(
            &response,
            &expected_challenge,
        ) {
            return Err(
                "Peer HandshakeAck doğrulaması başarısız"
                    .into(),
            );
        }

        Ok(stream)
    }

    pub async fn accept_authenticated_request(
        listener: &TcpListener,
        wallet: &Wallet,
    ) -> Result<
        (
            TcpStream,
            String,
            NetworkMessage,
            NetworkMessage,
        ),
        String,
    > {
        let (
            mut stream,
            peer_address,
            handshake,
        ) =
            Self::accept_authenticated(
                listener,
                wallet,
            )
            .await?;

        let request =
            Self::read_message(
                &mut stream,
            )
            .await?;

        Ok((
            stream,
            peer_address,
            handshake,
            request,
        ))
    }

    pub async fn accept_authenticated(
        listener: &TcpListener,
        wallet: &Wallet,
    ) -> Result<
        (
            TcpStream,
            String,
            NetworkMessage,
        ),
        String,
    > {
        let (
            mut stream,
            peer_address,
        ) =
            Self::accept_connection(
                listener,
            )
            .await?;

        let handshake =
            Self::read_message(
                &mut stream,
            )
            .await?;

        let challenge =
            Network::handshake_challenge(
                &handshake,
            )
            .unwrap_or("")
            .to_string();

        let mut validation_network =
            Network::new();

        validation_network.receive(
            handshake.clone(),
        );

        let accepted =
            validation_network
                .identified_peer_count()
                == 1;

        let handshake_ack =
            Network::create_handshake_ack(
                wallet,
                accepted,
                challenge,
            );

        Self::send_message(
            &mut stream,
            &handshake_ack,
        )
        .await?;

        if !accepted {
            return Err(
                "Peer handshake doğrulaması başarısız"
                    .into(),
            );
        }

        Ok((
            stream,
            peer_address,
            handshake,
        ))
    }

    pub async fn send_and_receive(
        address: &str,
        message: &NetworkMessage,
    ) -> Result<NetworkMessage, String> {
        let mut stream =
            Self::connect(
                address,
            )
            .await?;

        Self::send_message(
            &mut stream,
            message,
        )
        .await?;

        Self::read_message(
            &mut stream,
        )
        .await
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
