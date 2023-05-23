#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(dead_code)]

use num_derive::FromPrimitive;
use strum::Display;
use thiserror::Error;

include!(concat!(env!("OUT_DIR"), "/libmpv.rs"));
