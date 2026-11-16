//! Lets mpv read the file another peer offers. mpv opens a location of the
//! protocol registered here through its stream callbacks, and reads it on
//! a thread of its own through a [`Reader`].

use std::ffi::{CStr, c_char, c_int, c_void};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use anyhow::Result;
use niketsu_core::chunk::{FetchRoute, SharedFile};
use niketsu_core::communicator::PeerId;
use niketsu_core::stream::{Cancel, ReadError, Reader};

use crate::MpvHandle;
use crate::bindings::*;

const PROTOCOL: &CStr = c"niketsu";

/// What mpv is given to load a stream. Which stream it is, the adapter
/// keeps: the one it last offered.
pub const LOCATION: &CStr = c"niketsu://stream";

/// The streams mpv opens: where their chunks come from, and of which file
/// the next one is.
#[derive(Debug)]
pub struct Streams {
    route: FetchRoute,
    offered: Mutex<Option<Offer>>,
}

/// A file mpv was given to play as a stream.
#[derive(Debug, Clone)]
struct Offer {
    provider: PeerId,
    file: SharedFile,
    failed: Arc<AtomicBool>,
}

/// One stream mpv has open: the cookie of its callbacks. mpv reads, seeks
/// and asks for the size on one thread, and cancels from another.
struct Stream {
    reader: Mutex<Reader>,
    cancel: Arc<Cancel>,
    /// Set when a read failed for want of a chunk, which mpv itself takes
    /// for the end of the file.
    failed: Arc<AtomicBool>,
}

impl Streams {
    /// Registers the protocol with `mpv`. mpv keeps the pointer to the
    /// streams until it is destroyed, so they have to outlive it.
    pub fn register(mpv: MpvHandle, route: FetchRoute) -> Result<Arc<Self>> {
        let streams = Arc::new(Self {
            route,
            offered: Mutex::default(),
        });
        let user_data = Arc::as_ptr(&streams) as *mut c_void;
        let ret = unsafe { mpv_stream_cb_add_ro(mpv.0, PROTOCOL.as_ptr(), user_data, Some(open)) };
        mpv_error::try_from(ret)?.ok()?;
        Ok(streams)
    }

    /// The next stream mpv opens is of `file` as `provider` offers it.
    /// Returns what tells whether one of its reads failed.
    pub fn offer(&self, provider: PeerId, file: SharedFile) -> Arc<AtomicBool> {
        let failed = Arc::new(AtomicBool::new(false));
        let offer = Offer {
            provider,
            file,
            failed: failed.clone(),
        };
        *self.offered.lock().unwrap_or_else(PoisonError::into_inner) = Some(offer);
        failed
    }

    fn open(&self) -> Option<Stream> {
        let offered = self.offered.lock().unwrap_or_else(PoisonError::into_inner);
        let offer = offered.clone()?;
        let cancel = Arc::new(Cancel::default());
        let route = self.route.clone();
        let reader = Reader::of(&offer.file, offer.provider, route, cancel.clone());
        Some(Stream {
            reader: Mutex::new(reader),
            cancel,
            failed: offer.failed,
        })
    }
}

impl Stream {
    fn reader(&self) -> std::sync::MutexGuard<'_, Reader> {
        self.reader.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

unsafe extern "C" fn open(
    user_data: *mut c_void,
    _uri: *mut c_char,
    info: *mut mpv_stream_cb_info,
) -> c_int {
    let streams = unsafe { &*(user_data as *const Streams) };
    let Some(stream) = streams.open() else {
        return mpv_error::MPV_ERROR_LOADING_FAILED as c_int;
    };
    let info = unsafe { &mut *info };
    info.cookie = Box::into_raw(Box::new(stream)) as *mut c_void;
    info.read_fn = Some(read);
    info.seek_fn = Some(seek);
    info.size_fn = Some(size);
    info.close_fn = Some(close);
    info.cancel_fn = Some(cancel);
    0
}

unsafe extern "C" fn read(cookie: *mut c_void, buf: *mut c_char, nbytes: u64) -> i64 {
    let stream = unsafe { &*(cookie as *const Stream) };
    let buf = unsafe { std::slice::from_raw_parts_mut(buf as *mut u8, nbytes as usize) };
    match stream.reader().read(buf) {
        Ok(len) => len as i64,
        Err(ReadError::Cancelled) => -1,
        Err(ReadError::Failed) => {
            stream.failed.store(true, Ordering::Release);
            -1
        }
    }
}

unsafe extern "C" fn seek(cookie: *mut c_void, offset: i64) -> i64 {
    let stream = unsafe { &*(cookie as *const Stream) };
    let Ok(to) = u64::try_from(offset) else {
        return mpv_error::MPV_ERROR_GENERIC as i64;
    };
    stream.reader().seek(to);
    offset
}

unsafe extern "C" fn size(cookie: *mut c_void) -> i64 {
    let stream = unsafe { &*(cookie as *const Stream) };
    stream.reader().size() as i64
}

unsafe extern "C" fn close(cookie: *mut c_void) {
    drop(unsafe { Box::from_raw(cookie as *mut Stream) });
}

/// Called while another thread may wait in `read`, which holds the reader.
unsafe extern "C" fn cancel(cookie: *mut c_void) {
    let stream = unsafe { &*(cookie as *const Stream) };
    stream.cancel.cancel();
}
