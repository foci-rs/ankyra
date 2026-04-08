//! Reserved protocol names, IDs, and emission for the built-in bootstrap
//! items.
//!
//! Task 10 introduced the three reserved names and two fixed ID slots so
//! the sort stage can synthesize them alongside user items. Task 12 grows
//! this module with the matching emission: the `IdentifyResponse` reply
//! struct, the `handle_identify` dispatch helper that answers the host's
//! `identify` command by streaming slices of the compressed data
//! dictionary, and the Klipper-accurate format strings the dictionary
//! builder uses for the three synthesized items.

#![allow(dead_code)]

use proc_macro2::TokenStream as TokenStream2;
use quote::quote;

/// Host-facing name of the bootstrap command. The host always sends id 1
/// to retrieve the data dictionary.
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

/// Klipper-accurate message format for the `identify` command. The host
/// sends `(offset: u32, count: u32)` and expects a response sliced at
/// that offset.
pub(crate) fn identify_cmd_format() -> &'static str {
    "identify offset=%u count=%u"
}

/// Klipper-accurate message format for the `identify_response` reply.
/// The reply carries `(offset: u32, data: &[u8])` where `data` is a slice
/// of the raw dictionary bytes.
pub(crate) fn identify_response_reply_format() -> &'static str {
    "identify_response offset=%u data=%.*s"
}

/// Klipper-accurate message format for the `shutdown` reply — matches the
/// [`ankyra::Shutdown`](crate::shutdown) payload wire layout.
pub(crate) fn shutdown_reply_format() -> &'static str {
    "shutdown clock=%u static_string_id=%hu"
}

/// Emit the `IdentifyResponse` reply struct, its `SendReply` hook, and the
/// `handle_identify` dispatch fn.
///
/// Placing all three together mirrors how a user-level `#[klipper_reply]`
/// struct would look, minus the carrier macro (which is not needed —
/// `IdentifyResponse` is synthesized, not cross-crate).
///
/// The emitted code makes two assumptions that the rest of the generated
/// module upholds:
///
/// 1. `DICT_BYTES: &[u8; _]` is a sibling item (emitted by
///    [`crate::dictionary::emit`]).
/// 2. `Sender` is a unit struct implementing
///    `SendReply<IdentifyResponse>` with hardcoded id 0 (emitted by
///    [`crate::senders::emit`]).
pub(crate) fn emit() -> TokenStream2 {
    quote! {
        /// Reply payload for the built-in `identify` command.
        ///
        /// Carries a `(offset, data)` pair sliced from the dictionary bytes,
        /// matching the Klipper `identify_response offset=%u data=%.*s`
        /// message format.
        pub struct IdentifyResponse<'a> {
            pub offset: u32,
            pub data: &'a [u8],
        }

        impl ::ankyra::reply::ReplyPayload for IdentifyResponse<'_> {}

        impl ::ankyra::encoding::Writable for IdentifyResponse<'_> {
            fn write(&self, output: &mut impl ::ankyra::OutputBuffer) {
                <u32 as ::ankyra::encoding::Writable>::write(&self.offset, output);
                <&[u8] as ::ankyra::encoding::Writable>::write(&self.data, output);
            }
        }

        /// Dispatch handler for the built-in `identify` command.
        ///
        /// Reads `(offset, count)` off the frame, clips them against the
        /// dictionary bounds, and emits a single `IdentifyResponse` slice
        /// back to the host. The host iterates with monotonically
        /// increasing `offset` values until it receives an empty `data`
        /// slice.
        fn handle_identify<S>(
            frame: &mut &[u8],
            sender: &mut S,
        ) -> ::core::result::Result<(), ::ankyra::encoding::ReadError>
        where
            S: for<'a> ::ankyra::SendReply<IdentifyResponse<'a>>,
        {
            let offset = <u32 as ::ankyra::encoding::Readable>::read(frame)?;
            let count = <u32 as ::ankyra::encoding::Readable>::read(frame)?;
            let bytes = DICT_BYTES.as_slice();
            let off = (offset as usize).min(bytes.len());
            let end = off.saturating_add(count as usize).min(bytes.len());
            let payload = IdentifyResponse {
                offset,
                data: &bytes[off..end],
            };
            <S as ::ankyra::SendReply<IdentifyResponse<'_>>>::send(sender, payload);
            ::core::result::Result::Ok(())
        }
    }
}
