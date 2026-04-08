//! Reserved protocol names and IDs owned by the assembler.
//!
//! Task 10 fixes the three names and two ID slots that are visible to the
//! rest of the sorter. Task 12 will grow this module to house the
//! `handle_identify` dispatch helper and the `IdentifyResponse` reply
//! descriptor that together answer a host's `identify` command. The seeds
//! below exist now so Task 10's `sort::assemble` can reference canonical
//! constants instead of duplicating the literals inline.

#![allow(dead_code)]

/// Host-facing name of the bootstrap command. The host always sends id 1
/// to retrieve the compressed data dictionary.
pub(crate) const IDENTIFY_CMD_NAME: &str = "identify";

/// Host-facing name of the identify reply. Always id 0.
pub(crate) const IDENTIFY_RESPONSE_REPLY_NAME: &str = "identify_response";

/// Host-facing name of the shutdown reply. Its id is chosen by the
/// canonical sort like any other reply.
pub(crate) const SHUTDOWN_REPLY_NAME: &str = "shutdown";

/// Fixed command id for `identify`.
pub(crate) const IDENTIFY_CMD_ID: u16 = 1;

/// Fixed reply id for `identify_response`.
pub(crate) const IDENTIFY_RESPONSE_REPLY_ID: u16 = 0;
