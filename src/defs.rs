pub const LOCAL_SOCKET_ADDR: &'static str = "0.0.0.0:6881";
pub const LOCAL_PORT: u16 = 6881;
pub const PEER_ID: &'static [u8; 20] = b"M0-0-1--IT2022------";
pub const CONNECT_MAGIC_NUMBER: u64 = 0x41727101980;
pub const ACTION_CONNECT: u32 = 0;
pub const ACTION_ANNOUNCE: u32 = 1;
pub const ACTION_SCRAPE: u32 = 2;
pub const ACTION_ERROR: u32 = 3;
pub const ANNOUNCE_EVENT_NONE: u32 = 0;
pub const ANNOUNCE_EVENT_COMPLETED: u32 = 1;
pub const ANNOUNCE_EVENT_STARTED: u32 = 2;
pub const ANNOUNCE_EVENT_STOPPED: u32 = 3;
pub const MAX_PEERS: i32 = 16;
pub const HANDSHAKE_PSTR: &'static [u8; 19] = b"BitTorrent protocol";
pub const FRAGMENT_SIZE: usize = 1 << 14;
pub const MSG_BUF_SIZE: usize = FRAGMENT_SIZE + 1024; // Some extra buffer in case multiple messages are coming in.
