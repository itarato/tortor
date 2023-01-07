pub static LOCAL_SOCKET_ADDR: &'static str = "0.0.0.0:6881";
pub static LOCAL_PORT: u16 = 6881;
pub static PEER_ID: &'static [u8; 20] = b"M0-0-1--IT2022------";
pub static CONNECT_MAGIC_NUMBER: u64 = 0x41727101980;
pub static ACTION_CONNECT: u32 = 0;
pub static ACTION_ANNOUNCE: u32 = 1;
pub static ACTION_SCRAPE: u32 = 2;
pub static ACTION_ERROR: u32 = 3;
pub static ANNOUNCE_EVENT_NONE: u32 = 0;
pub static ANNOUNCE_EVENT_COMPLETED: u32 = 1;
pub static ANNOUNCE_EVENT_STARTED: u32 = 2;
pub static ANNOUNCE_EVENT_STOPPED: u32 = 3;
pub static MAX_PEERS: i32 = 16;

pub const HANDSHAKE_PSTR: &'static [u8; 19] = b"BitTorrent protocol";
pub const PIECE_SIZE: usize = 1 << 14;
pub const MSG_BUF_SIZE: usize = PIECE_SIZE + 1024; // Some extra buffer in case multiple messages are coming in.
