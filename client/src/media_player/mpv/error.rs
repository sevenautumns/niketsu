use anyhow::bail;
use num_traits::FromPrimitive;

use crate::media_player::mpv::bindings::*;

impl TryFrom<i32> for mpv_error {
    type Error = anyhow::Error;

    fn try_from(value: i32) -> Result<Self, Self::Error> {
        match FromPrimitive::from_i32(value) {
            Some(val) => Ok(val),
            None => bail!("Could not parse mpv_error: {value}"),
        }
    }
}

impl TryFrom<mpv_error> for () {
    type Error = mpv_error;

    fn try_from(ret: mpv_error) -> Result<Self, Self::Error> {
        match ret {
            mpv_error::MPV_ERROR_SUCCESS => Ok(()),
            e => Err(e),
        }
    }
}
