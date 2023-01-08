use std::error::Error;
use std::io;
use std::sync::atomic::AtomicI8;
use std::sync::Arc;
use std::time::Duration;

use num_traits::FromPrimitive;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::{ReadHalf, WriteHalf};
use tokio::net::TcpStream;
use tokio::sync::Mutex;

use crate::defs::*;
use crate::download::*;
use crate::macros::*;
use crate::IP;

#[derive(Debug)]
pub struct MessageBuf {
    buf: Vec<u8>,
    start: usize,
    end: usize,
    min_available: usize,
}

/**
 * [=======X-------X        ]
 *    start^       ^end     ^len
 *         <current>
 *                  <buffer>
 */
impl MessageBuf {
    pub fn new_with_min_available(min_available: usize) -> Self {
        Self {
            buf: vec![0; min_available],
            start: 0,
            end: 0,
            min_available,
        }
    }

    pub fn buffer(&mut self) -> &mut [u8] {
        &mut self.buf[self.end..]
    }

    pub fn current(&self) -> &[u8] {
        &self.buf[self.start..self.end]
    }

    pub fn clear(&mut self) {
        self.buf.clear();
        self.start = 0;
        self.end = 0;
    }

    fn consume_message(&mut self, delta: usize) {
        assert!(
            delta <= self.end - self.start,
            "Consumed more than available"
        );
        self.start += delta;
    }

    fn register_read_len(&mut self, read_len: usize) {
        self.end += read_len;
    }

    pub fn ensure_read_capacity(&mut self) {
        if self.start == self.end && self.start > 0 {
            self.clear();
            return;
        }

        if self.start + self.min_available <= self.buf.len() {
            return;
        }

        let additional_space = 0usize.max(self.start + self.min_available - self.buf.len());
        self.buf.reserve(additional_space);
        self.buf.resize(self.buf.capacity(), 0);

        log::info!(
            "Reserved {} more bytes in read buffer (now {})",
            additional_space,
            self.buf.len()
        );
    }
}

#[derive(FromPrimitive, ToPrimitive, Debug)]
enum PeerMessageCode {
    Choke,
    Unchoke,
    Interested,
    NotInterested,
    Have,
    Bitfield,
    Request,
    Piece,
    Cancel,
    Port,
}

impl TryFrom<&[u8]> for PeerMessageCode {
    type Error = ();

    fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
        if value.len() < 5 {
            log::error!("No 5 bytes");
            return Err(());
        }

        let b = u8::from_be_bytes(value[4..5].try_into().unwrap());
        FromPrimitive::from_u8(b).ok_or(())
    }
}

enum PeerState {
    // Need to initiate handshake.
    NeedHandshake,
    // Need to unchoke itself.
    Choked,
    // Ready to send out requests for pieces and receive them.
    Pulling,
    // Completed.
    Dead,
}

pub struct Peer {
    idx: usize,
    ip: IP,
    info_hash: Arc<Vec<u8>>,
    download: Arc<Mutex<Download>>,
}

impl Peer {
    pub fn new(
        idx: usize,
        ip: IP,
        info_hash: Arc<Vec<u8>>,
        download: Arc<Mutex<Download>>,
    ) -> Self {
        Self {
            idx,
            ip,
            info_hash,
            download,
        }
    }

    pub async fn exec(&self) {
        let state = Arc::new(Mutex::new(PeerState::NeedHandshake));

        let mut stream = match TcpStream::connect(self.ip.socket_addr()).await {
            Ok(stream) => stream,
            Err(err) => {
                log::error!("{} Peer connection failed: {:?}", self.idx, err);
                return;
            }
        };

        let mut read_buffer = MessageBuf::new_with_min_available(MSG_BUF_SIZE);

        if let Err(err) = self.handshake(&mut stream, &mut read_buffer).await {
            log::error!("{} Handshake error: {:?}", self.idx, err);
            return;
        }

        *(state.lock().await) = PeerState::Choked;

        let (mut reader, mut writer) = stream.split();
        let reader_state = state.clone();
        let writer_state = state;

        let writer_task = async move {
            if Peer::interested(&mut writer).await.is_err() {
                log::warn!("{} Failed sending interested message", self.idx);
                return;
            }

            let mut pull_complete = false;

            loop {
                match *writer_state.lock().await {
                    PeerState::NeedHandshake => panic!("Unexpected state"),
                    PeerState::Dead => break,
                    PeerState::Pulling => {
                        if pull_complete {
                            tokio::time::sleep(Duration::from_secs(1)).await;
                            tokio::task::yield_now().await;
                            continue;
                        }

                        let mut download = self.download.lock().await;
                        let next_fragment_info = download.next_not_requested_fragment();

                        if next_fragment_info.is_none() {
                            pull_complete = true;
                            continue;
                        }

                        let (piece_idx, fragment_idx) = next_fragment_info.unwrap();
                        download.pieces[piece_idx].mark_requested(fragment_idx);

                        if Peer::request(
                            &mut writer,
                            piece_idx as u32,
                            (fragment_idx * FRAGMENT_SIZE) as u32,
                        )
                        .await
                        .is_err()
                        {
                            log::error!("{} Request message error", self.idx);
                            return;
                        }

                        tokio::time::sleep(Duration::from_secs(3)).await;
                    }
                    PeerState::Choked => {
                        log::info!("{} Still in choked state - yielding", self.idx);
                        // tokio::task::yield_now().await;
                        tokio::time::sleep(Duration::from_secs(1)).await;
                    }
                };
            }
        };

        let reader_task = async move {
            loop {
                log::info!("{} Listening for incoming stream", self.idx);

                if let Err(err) = self.read_one_message(&mut reader, &mut read_buffer).await {
                    log::error!("{} Peer message read error: {:?}", self.idx, err);
                    *(reader_state.lock().await) = PeerState::Dead;
                    return;
                }

                let last_msg = self.take_last_message(&mut read_buffer);
                log::info!("{} Got post-handshake message: {:?}", self.idx, last_msg);

                match last_msg {
                    Some(PeerMessage::Bitfield(_bitfields)) => {
                        // FIXME: use bitfields to know what is available
                        log::info!(
                            "{} Bitfields are in (lets pretend we recorded it)",
                            self.idx
                        );
                        // *(reader_state.lock().await) = PeerState::Pulling;
                    }
                    Some(PeerMessage::KeepAlive) => {
                        log::info!("{} KeepAlive", self.idx);
                        // tokio::task::yield_now().await;
                        tokio::time::sleep(Duration::from_secs(1)).await;
                    }
                    Some(PeerMessage::Unchoke) => {
                        log::info!("{} Unchoked", self.idx);
                        *(reader_state.lock().await) = PeerState::Pulling;
                    }
                    Some(PeerMessage::Piece(index, begin)) => {
                        log::info!(
                            "{} Piece came in for piece #{} offset #{}",
                            self.idx,
                            index,
                            begin
                        );
                    }
                    None => {
                        log::error!("{} Invalid post handshake message", self.idx);
                        *(reader_state.lock().await) = PeerState::Dead;
                        return;
                    }
                    _ => {
                        log::error!(
                            "{} Message type not supported yet: {:?}",
                            self.idx,
                            last_msg
                        );
                        *(reader_state.lock().await) = PeerState::Dead;
                        return;
                    }
                }
            }
        };

        tokio::select! {
            _ = writer_task => {}
            _ = reader_task => {}
        };

        log::info!("{} Peer closing", self.idx);
    }

    pub async fn handshake(
        &self,
        stream: &mut TcpStream,
        read_buffer: &mut MessageBuf,
    ) -> Result<(), Box<dyn Error>> {
        let mut buf: Vec<u8> = vec![];

        to_buf!(HANDSHAKE_PSTR.len() as u8, buf);
        vec_to_buf!(HANDSHAKE_PSTR, buf);
        vec_to_buf!(&[0u8; 8][..], buf);
        vec_to_buf!(&self.info_hash[..], buf);
        vec_to_buf!(&PEER_ID[..], buf);

        assert_eq!(self.handhake_len(), buf.len());

        log::info!("Handshake init with: {:?}", self.ip);
        stream.write_all(&buf[..]).await?;
        log::info!("Handshake sent");

        self.read_handshake(stream, read_buffer).await?;

        log::info!("Handshake waiting for response");
        let resp_buf = read_buffer.current();

        log::info!("Got handshake response");

        if resp_buf.len() < self.handhake_len() {
            log::warn!(
                "Handshake response is incorrect: {} != {}",
                resp_buf.len(),
                self.handhake_len()
            );
            return Err("Incorrect handshake response".into());
        }

        if resp_buf[0] != HANDSHAKE_PSTR.len() as u8 {
            log::warn!("Handshake response PSTR len is incorrect: {}", resp_buf[0]);
            return Err("Incorrect handshake response".into());
        }

        if resp_buf[1..1 + HANDSHAKE_PSTR.len()] != buf[1..1 + HANDSHAKE_PSTR.len()] {
            log::warn!("Handshake PSTR is not matching");
            return Err("Incorrect handshake response".into());
        }

        if resp_buf[28..48] != buf[28..48] {
            log::warn!("Handshake info hash is not matching");
            return Err("Incorrect handshake response".into());
        }

        read_buffer.consume_message(self.handhake_len());

        Ok(())
    }

    fn handhake_len(&self) -> usize {
        49 + HANDSHAKE_PSTR.len()
    }

    async fn read_one_message(
        &self,
        stream: &mut ReadHalf<'_>,
        read_buffer: &mut MessageBuf,
    ) -> Result<(), io::Error> {
        read_buffer.ensure_read_capacity();

        loop {
            if self.buffer_has_full_message(read_buffer) {
                log::info!("{} got full message", self.idx);
                break;
            }

            log::info!(
                "{} Waiting read into {} byte buf",
                self.idx,
                read_buffer.buffer().len()
            );
            let read_len = stream.read(&mut read_buffer.buffer()).await?;
            read_buffer.register_read_len(read_len);
            log::info!("{} Read {} bytes", self.idx, read_len);

            if read_buffer.current().len() == 0 && read_len == 0 {
                log::info!("{} Got Keep-Alive", self.idx);
                break;
            }
        }

        Ok(())
    }

    async fn read_handshake(
        &self,
        stream: &mut TcpStream,
        read_buffer: &mut MessageBuf,
    ) -> Result<(), io::Error> {
        read_buffer.ensure_read_capacity();

        loop {
            if read_buffer.current().len() >= self.handhake_len() {
                log::info!("{} got full handshake", self.idx);
                break;
            }

            log::info!(
                "{} Waiting read into {} byte buf",
                self.idx,
                read_buffer.buffer().len()
            );
            let read_len = stream.read(&mut read_buffer.buffer()).await?;
            read_buffer.register_read_len(read_len);
            log::info!("{} Read {} bytes", self.idx, read_len);

            if read_buffer.current().len() == 0 && read_len == 0 {
                log::info!("{} Got Keep-Alive", self.idx);
                break;
            }
        }

        Ok(())
    }

    fn buffer_has_full_message(&self, read_buffer: &mut MessageBuf) -> bool {
        let current = read_buffer.current();
        if current.len() < 4
        /* u32 len + u8 action */
        {
            log::info!("{} has {} bytes only", self.idx, current.len());
            return false;
        }

        let msg_len = u32::from_be_bytes(current[..4].try_into().unwrap()) as usize;
        log::info!(
            "{} Msg is {} bytes and we got {} already",
            self.idx,
            msg_len,
            current.len()
        );

        if msg_len == 0 {
            log::error!("{} Invalid message length: {}", self.idx, msg_len);
            dbg!(current);
            panic!("Cannot handle invalid message length!");
        }

        current.len() >= msg_len + 4
    }

    fn take_last_message(&self, read_buffer: &mut MessageBuf) -> Option<PeerMessage> {
        let current = read_buffer.current();

        if current.len() == 0 {
            return Some(PeerMessage::KeepAlive);
        }

        if current.len() < 5 {
            log::warn!("{} Invalid message len: {}", self.idx, current.len());
            return None;
        }

        let msg_len = u32::from_be_bytes(current[..4].try_into().unwrap()) as usize;
        let message_raw = &current[4..msg_len + 4];
        assert_eq!(msg_len, message_raw.len());

        let message = match message_raw[0] {
            0 => Some(PeerMessage::Choke),
            1 => Some(PeerMessage::Unchoke),
            2 => Some(PeerMessage::Interested),
            3 => Some(PeerMessage::NotInterested),
            4 => {
                let piece = u32::from_be_bytes(message_raw[1..5].try_into().unwrap());
                Some(PeerMessage::Have(piece))
            }
            5 => {
                let bitfields = message_raw[1..msg_len - 1].to_vec();
                Some(PeerMessage::Bitfield(bitfields))
            }
            6 => {
                let index = u32::from_be_bytes(message_raw[1..5].try_into().unwrap());
                let begin = u32::from_be_bytes(message_raw[5..9].try_into().unwrap());
                let length = u32::from_be_bytes(message_raw[9..13].try_into().unwrap());
                Some(PeerMessage::Request(index, begin, length))
            }
            7 => {
                // FIXME: keep content.
                let index = u32::from_be_bytes(message_raw[1..5].try_into().unwrap());
                let begin = u32::from_be_bytes(message_raw[5..9].try_into().unwrap());
                Some(PeerMessage::Piece(index, begin))
            }
            8 => {
                let index = u32::from_be_bytes(message_raw[1..5].try_into().unwrap());
                let begin = u32::from_be_bytes(message_raw[5..9].try_into().unwrap());
                let length = u32::from_be_bytes(message_raw[9..13].try_into().unwrap());
                Some(PeerMessage::Cancel(index, begin, length))
            }
            9 => {
                let port = u16::from_be_bytes(message_raw[1..3].try_into().unwrap());
                Some(PeerMessage::Port(port))
            }
            _ => unimplemented!("Message type {} not handled yet", message_raw[0]),
        };

        read_buffer.consume_message(4 + msg_len);

        message
    }

    pub async fn request(
        stream: &mut WriteHalf<'_>,
        piece_index: u32,
        piece_offset: u32,
    ) -> Result<(), io::Error> {
        let mut buf_out: Vec<u8> = vec![];

        to_buf!(13u32, buf_out);
        to_buf!(6u8, buf_out);
        to_buf!(piece_index, buf_out);
        to_buf!(piece_offset, buf_out);
        to_buf!(FRAGMENT_SIZE as u32, buf_out);

        log::info!(
            "Sent: request for piece #{} and offset #{}",
            piece_index,
            piece_offset
        );
        stream.write_all(&mut buf_out).await?;

        Ok(())
    }

    pub async fn interested(stream: &mut WriteHalf<'_>) -> Result<(), io::Error> {
        let mut buf_out: Vec<u8> = vec![];

        to_buf!(1u32, buf_out);
        to_buf!(2u8, buf_out);

        log::info!("Sent: interested");
        stream.write_all(&mut buf_out).await?;

        Ok(())
    }
}

#[derive(Debug)]
pub enum PeerMessage {
    KeepAlive,
    Choke,
    Unchoke,
    Interested,
    NotInterested,
    Have(u32),              // Piece index.
    Bitfield(Vec<u8>),      // Bitfields.
    Request(u32, u32, u32), // Index, begin, length.
    Piece(u32, u32),        // Index, begin.
    Cancel(u32, u32, u32),  // Index, begin, length.
    Port(u16),              // Port.
}
