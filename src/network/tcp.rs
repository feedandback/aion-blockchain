use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use tokio::sync::{Semaphore, mpsc};
use tokio::time::timeout;

use crate::network::{Network, NetworkMessage, ONE_SHOT_CLIENT_LISTEN_ADDRESS};
use crate::protocol::{
    MAX_CONCURRENT_NETWORK_CONNECTIONS, MAX_NETWORK_MESSAGE_BYTES, NETWORK_CONNECT_TIMEOUT_SECONDS,
    NETWORK_IO_TIMEOUT_SECONDS,
};
use crate::wallet::Wallet;

pub struct TcpTransport;

#[allow(dead_code)]
impl TcpTransport {
    pub async fn bind(address: &str) -> Result<TcpListener, String> {
        TcpListener::bind(address)
            .await
            .map_err(|error| format!("TCP listener could not be started: {}", error))
    }

    pub async fn connect(address: &str) -> Result<TcpStream, String> {
        timeout(
            Duration::from_secs(NETWORK_CONNECT_TIMEOUT_SECONDS),
            TcpStream::connect(address),
        )
        .await
        .map_err(|_| {
            format!(
                "Peer connection timed out after {} seconds",
                NETWORK_CONNECT_TIMEOUT_SECONDS
            )
        })?
        .map_err(|error| format!("Peer connection could not be established: {}", error))
    }

    pub async fn accept_connection(listener: &TcpListener) -> Result<(TcpStream, String), String> {
        let (stream, peer_address) = listener
            .accept()
            .await
            .map_err(|error| format!("TCP peer could not be accepted: {}", error))?;

        Ok((stream, peer_address.to_string()))
    }

    pub async fn send_message(
        stream: &mut TcpStream,
        message: &NetworkMessage,
    ) -> Result<(), String> {
        let payload = serde_json::to_vec(message).map_err(|error| {
            format!("Network message could not be serialized to JSON: {}", error)
        })?;

        if payload.is_empty() {
            return Err("Empty network message cannot be sent".into());
        }

        if payload.len() > MAX_NETWORK_MESSAGE_BYTES {
            return Err("Network message byte limit exceeded".into());
        }

        let payload_length = u32::try_from(payload.len())
            .map_err(|_| "Network message length exceeds the u32 range".to_string())?;

        timeout(
            Duration::from_secs(NETWORK_IO_TIMEOUT_SECONDS),
            stream.write_all(&payload_length.to_be_bytes()),
        )
        .await
        .map_err(|_| "Network message header send timed out".to_string())?
        .map_err(|error| format!("Network message header could not be sent: {}", error))?;

        timeout(
            Duration::from_secs(NETWORK_IO_TIMEOUT_SECONDS),
            stream.write_all(&payload),
        )
        .await
        .map_err(|_| "Network message send timed out".to_string())?
        .map_err(|error| format!("Network message could not be sent: {}", error))?;

        timeout(
            Duration::from_secs(NETWORK_IO_TIMEOUT_SECONDS),
            stream.flush(),
        )
        .await
        .map_err(|_| "Network message flush timed out".to_string())?
        .map_err(|error| format!("Network message could not be flushed: {}", error))?;

        Ok(())
    }

    pub async fn read_message(stream: &mut TcpStream) -> Result<NetworkMessage, String> {
        Self::read_message_or_eof(stream)
            .await?
            .ok_or_else(|| "Network connection closed while waiting for a message".to_string())
    }

    pub async fn read_message_or_eof(
        stream: &mut TcpStream,
    ) -> Result<Option<NetworkMessage>, String> {
        let mut length_bytes = [0u8; 4];

        let frame_started = timeout(Duration::from_secs(NETWORK_IO_TIMEOUT_SECONDS), async {
            let first_byte_count = stream.read(&mut length_bytes[..1]).await?;

            if first_byte_count == 0 {
                return Ok::<bool, std::io::Error>(false);
            }

            stream.read_exact(&mut length_bytes[1..]).await?;

            Ok(true)
        })
        .await
        .map_err(|_| "Network message header read timed out".to_string())?
        .map_err(|error| format!("Network message header could not be read: {}", error))?;

        if !frame_started {
            return Ok(None);
        }

        let payload_length = u32::from_be_bytes(length_bytes) as usize;

        if payload_length == 0 {
            return Err("Empty network message was rejected".into());
        }

        if payload_length > MAX_NETWORK_MESSAGE_BYTES {
            return Err("Network message byte limit exceeded".into());
        }

        let mut payload = vec![0u8; payload_length];

        timeout(
            Duration::from_secs(NETWORK_IO_TIMEOUT_SECONDS),
            stream.read_exact(&mut payload),
        )
        .await
        .map_err(|_| "Network message read timed out".to_string())?
        .map_err(|error| format!("Network message could not be read: {}", error))?;

        let message = serde_json::from_slice(&payload)
            .map_err(|error| format!("Network message JSON format is invalid: {}", error))?;

        Ok(Some(message))
    }

    pub async fn run_listener(
        address: &str,
        sender: mpsc::Sender<(NetworkMessage, String)>,
    ) -> Result<(), String> {
        let listener = Self::bind(address).await?;

        let connection_limit = Arc::new(Semaphore::new(MAX_CONCURRENT_NETWORK_CONNECTIONS));

        println!(" Kybernetes TCP listener active: {}", address);

        println!(
            " Maximum concurrent TCP connections: {}",
            MAX_CONCURRENT_NETWORK_CONNECTIONS
        );

        loop {
            if sender.is_closed() {
                return Err("TCP message channel closed".into());
            }

            let (mut stream, peer_address) = listener
                .accept()
                .await
                .map_err(|error| format!("TCP peer could not be accepted: {}", error))?;

            let permit = match connection_limit.clone().try_acquire_owned() {
                Ok(permit) => permit,

                Err(_) => {
                    println!(
                        " TCP connection rejected: concurrent connection limit ({}) reached",
                        MAX_CONCURRENT_NETWORK_CONNECTIONS
                    );

                    continue;
                }
            };

            let peer_address = peer_address.to_string();

            let message_sender = sender.clone();

            tokio::spawn(async move {
                let _permit = permit;

                let message = match Self::read_message(&mut stream).await {
                    Ok(message) => message,

                    Err(error) => {
                        println!(" TCP message rejected: {}", error);

                        return;
                    }
                };

                if message_sender.send((message, peer_address)).await.is_err() {
                    println!(" TCP message could not be forwarded: message channel closed");
                }
            });
        }
    }

    pub async fn run_authenticated_listener(
        address: &str,
        wallet: Arc<Wallet>,
        sender: mpsc::Sender<(NetworkMessage, String)>,
    ) -> Result<(), String> {
        let listener = Self::bind(address).await?;

        let connection_limit = Arc::new(Semaphore::new(MAX_CONCURRENT_NETWORK_CONNECTIONS));

        println!(
            " identity authenticated Kybernetes P2P listener active: {}",
            address
        );

        loop {
            if sender.is_closed() {
                return Err("P2P message channel closed".into());
            }

            let permit = connection_limit
                .clone()
                .acquire_owned()
                .await
                .map_err(|_| "P2P connection limiter closed".to_string())?;

            let (stream, peer_address) = Self::accept_connection(&listener).await?;

            let peer_wallet = wallet.clone();

            let message_sender = sender.clone();

            tokio::spawn(async move {
                let _permit = permit;

                let mut stream = stream;

                let handshake = match Self::read_message(&mut stream).await {
                    Ok(message) => message,

                    Err(error) => {
                        println!(" P2P handshake could not be read: {}", error);

                        return;
                    }
                };

                let challenge = Network::handshake_challenge(&handshake)
                    .unwrap_or("")
                    .to_string();

                let accepted = Network::validate_handshake(&handshake);

                let handshake_ack =
                    Network::create_handshake_ack(&peer_wallet, accepted, challenge);

                if let Err(error) = Self::send_message(&mut stream, &handshake_ack).await {
                    println!(" HandshakeAck could not be sent: {}", error);

                    return;
                }

                if !accepted {
                    println!(" identity unauthenticated peer rejected: {}", peer_address);

                    return;
                }

                println!(" Authenticated peer connected: {}", peer_address);

                if message_sender
                    .send((handshake, peer_address.clone()))
                    .await
                    .is_err()
                {
                    println!(" Peer identity could not be forwarded to the main node");

                    return;
                }

                loop {
                    let message = match Self::read_message_or_eof(&mut stream).await {
                        Ok(Some(message)) => message,

                        Ok(None) => {
                            break;
                        }

                        Err(error) => {
                            println!(" P2P message read error: {} ({})", peer_address, error);

                            break;
                        }
                    };

                    if message_sender
                        .send((message, peer_address.clone()))
                        .await
                        .is_err()
                    {
                        println!(" P2P message channel closed");

                        break;
                    }
                }
            });
        }
    }

    pub async fn broadcast_authenticated(
        peer_addresses: &[String],
        wallet: &Wallet,
        listen_address: &str,
        message: &NetworkMessage,
    ) -> (usize, usize) {
        let mut success_count = 0usize;

        let mut failure_count = 0usize;

        for peer_address in peer_addresses {
            match Self::send_authenticated_message(peer_address, wallet, listen_address, message)
                .await
            {
                Ok(()) => {
                    success_count += 1;

                    println!(" P2P broadcast sent: {}", peer_address);
                }

                Err(error) => {
                    failure_count += 1;

                    println!(" P2P broadcast failed: {} ({})", peer_address, error);
                }
            }
        }

        (success_count, failure_count)
    }

    pub async fn send_authenticated_message(
        address: &str,
        wallet: &Wallet,
        listen_address: &str,
        message: &NetworkMessage,
    ) -> Result<(), String> {
        let mut stream = Self::connect_authenticated(address, wallet, listen_address).await?;

        Self::send_message(&mut stream, message).await
    }

    pub async fn send_authenticated_request(
        address: &str,
        wallet: &Wallet,
        listen_address: &str,
        request: &NetworkMessage,
    ) -> Result<NetworkMessage, String> {
        let mut stream = Self::connect_authenticated(address, wallet, listen_address).await?;

        Self::send_message(&mut stream, request).await?;

        Self::read_message(&mut stream).await
    }

    pub async fn connect_authenticated(
        address: &str,
        wallet: &Wallet,
        listen_address: &str,
    ) -> Result<TcpStream, String> {
        let handshake = Network::create_handshake(wallet, listen_address.to_string());

        let expected_challenge = Network::handshake_challenge(&handshake)
            .ok_or_else(|| "Handshake challenge could not be created".to_string())?
            .to_string();

        let mut stream = Self::connect(address).await?;

        Self::send_message(&mut stream, &handshake).await?;

        let response = Self::read_message(&mut stream).await?;

        if !Network::validate_handshake_ack(&response, &expected_challenge) {
            return Err("Peer HandshakeAck validation failed".into());
        }

        Ok(stream)
    }

    pub async fn accept_authenticated_request(
        listener: &TcpListener,
        wallet: &Wallet,
    ) -> Result<(TcpStream, String, NetworkMessage, NetworkMessage), String> {
        let (mut stream, peer_address, handshake) =
            Self::accept_authenticated(listener, wallet).await?;

        let request = Self::read_message(&mut stream).await?;

        Ok((stream, peer_address, handshake, request))
    }

    pub async fn accept_authenticated(
        listener: &TcpListener,
        wallet: &Wallet,
    ) -> Result<(TcpStream, String, NetworkMessage), String> {
        let (stream, peer_address) = Self::accept_connection(listener).await?;

        Self::authenticate_incoming(stream, peer_address, wallet).await
    }

    pub async fn send_authenticated_client_message(
        address: &str,
        wallet: &Wallet,
        message: &NetworkMessage,
    ) -> Result<(), String> {
        Self::send_authenticated_message(address, wallet, ONE_SHOT_CLIENT_LISTEN_ADDRESS, message)
            .await
    }

    pub async fn authenticate_incoming(
        mut stream: TcpStream,
        peer_address: String,
        wallet: &Wallet,
    ) -> Result<(TcpStream, String, NetworkMessage), String> {
        let handshake = Self::read_message(&mut stream).await?;

        let challenge = Network::handshake_challenge(&handshake)
            .unwrap_or("")
            .to_string();

        let accepted = Network::validate_handshake(&handshake);

        let handshake_ack = Network::create_handshake_ack(wallet, accepted, challenge);

        Self::send_message(&mut stream, &handshake_ack).await?;

        if !accepted {
            return Err("Peer handshake validation failed".into());
        }

        Ok((stream, peer_address, handshake))
    }

    pub async fn send_and_receive(
        address: &str,
        message: &NetworkMessage,
    ) -> Result<NetworkMessage, String> {
        let mut stream = Self::connect(address).await?;

        Self::send_message(&mut stream, message).await?;

        Self::read_message(&mut stream).await
    }

    pub async fn send_to(address: &str, message: &NetworkMessage) -> Result<(), String> {
        let mut stream = Self::connect(address).await?;

        Self::send_message(&mut stream, message).await
    }

    pub async fn accept_one(listener: &TcpListener) -> Result<(NetworkMessage, String), String> {
        let (mut stream, peer_address) = listener
            .accept()
            .await
            .map_err(|error| format!("TCP peer could not be accepted: {}", error))?;

        let message = Self::read_message(&mut stream).await?;

        Ok((message, peer_address.to_string()))
    }
}
