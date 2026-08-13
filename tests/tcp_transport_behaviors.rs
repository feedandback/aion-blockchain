use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};

use kybernetes::network::tcp::TcpTransport;
use kybernetes::network::NetworkMessage;

async fn stream_after_client_writes(bytes: &[u8]) -> TcpStream {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("test listener must bind");
    let address = listener
        .local_addr()
        .expect("test listener must have a local address");
    let bytes = bytes.to_vec();

    let client = tokio::spawn(async move {
        let mut stream = TcpStream::connect(address)
            .await
            .expect("test client must connect");
        if !bytes.is_empty() {
            stream
                .write_all(&bytes)
                .await
                .expect("test bytes must be written");
        }
        stream
            .shutdown()
            .await
            .expect("test client must close cleanly");
    });

    let (stream, _) = listener
        .accept()
        .await
        .expect("test listener must accept the client");
    client
        .await
        .expect("test client task must complete");

    stream
}

#[tokio::test]
async fn clean_eof_is_a_normal_session_close() {
    let mut stream = stream_after_client_writes(&[]).await;

    let result = TcpTransport::read_message_or_eof(&mut stream).await;

    assert!(matches!(result, Ok(None)));
}

#[tokio::test]
async fn truncated_and_malformed_frames_remain_errors() {
    let mut partial_header_stream =
        stream_after_client_writes(&[0, 0]).await;
    let partial_header =
        TcpTransport::read_message_or_eof(&mut partial_header_stream).await;
    assert!(
        partial_header
            .expect_err("partial header must be rejected")
            .contains("başlığı okunamadı")
    );

    let mut partial_payload_frame = 8u32.to_be_bytes().to_vec();
    partial_payload_frame.extend_from_slice(b"{}");
    let mut partial_payload_stream =
        stream_after_client_writes(&partial_payload_frame).await;
    let partial_payload =
        TcpTransport::read_message_or_eof(&mut partial_payload_stream).await;
    assert!(
        partial_payload
            .expect_err("partial payload must be rejected")
            .contains("mesajı okunamadı")
    );

    let malformed_payload = b"not-json";
    let mut malformed_frame =
        u32::try_from(malformed_payload.len())
            .expect("test payload length must fit u32")
            .to_be_bytes()
            .to_vec();
    malformed_frame.extend_from_slice(malformed_payload);
    let mut malformed_stream =
        stream_after_client_writes(&malformed_frame).await;
    let malformed =
        TcpTransport::read_message_or_eof(&mut malformed_stream).await;
    assert!(
        malformed
            .expect_err("malformed JSON must be rejected")
            .contains("JSON formatı geçersiz")
    );
}

#[tokio::test]
async fn valid_frame_is_read_before_the_clean_session_close() {
    let message = NetworkMessage::SyncRequest;
    let payload = serde_json::to_vec(&message)
        .expect("test message must serialize");
    let mut frame =
        u32::try_from(payload.len())
            .expect("test payload length must fit u32")
            .to_be_bytes()
            .to_vec();
    frame.extend_from_slice(&payload);
    let mut stream = stream_after_client_writes(&frame).await;

    let received = TcpTransport::read_message_or_eof(&mut stream)
        .await
        .expect("valid frame must be read")
        .expect("valid frame must contain a message");
    assert!(matches!(received, NetworkMessage::SyncRequest));

    let close = TcpTransport::read_message_or_eof(&mut stream).await;
    assert!(matches!(close, Ok(None)));
}
