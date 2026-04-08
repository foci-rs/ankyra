//! Descriptors describing protocol items that providers register with the
//! assembler.
//!
//! Providers describe the commands, replies, outputs, and constants/enums
//! they export using these types. The assembler collects descriptors across
//! all providers, sorts them, assigns IDs, and produces both a dispatch
//! table and a Klipper data dictionary.
//!
//! Static strings are deliberately not represented here. Per the design
//! spec, static strings never cross the provider boundary; they are
//! registered firmware-locally via `ankyra_config!`.

use core::cmp::Ordering;

/// The protocol item kind that a [`MessageDescriptor`] represents.
///
/// Sort order is fixed by declaration order here: `Command < Reply <
/// Output`. The assembler relies on this ordering to produce deterministic
/// output.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum ItemKind {
    /// A command handled by the firmware (received from the host).
    Command,
    /// A reply emitted in response to a command.
    Reply,
    /// An unsolicited output (asynchronous message to the host).
    Output,
}

/// Descriptor for a protocol message: a command, reply, or output.
///
/// Providers produce a `&'static [MessageDescriptor]` at compile time. The
/// assembler sorts these by `(kind, protocol_name)`, deduplicates them,
/// and emits the dispatch table and data dictionary entries.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct MessageDescriptor {
    kind: ItemKind,
    protocol_name: &'static str,
    message_format: &'static str,
}

impl MessageDescriptor {
    /// Construct a descriptor for a command.
    #[must_use]
    pub const fn command(protocol_name: &'static str, message_format: &'static str) -> Self {
        Self {
            kind: ItemKind::Command,
            protocol_name,
            message_format,
        }
    }

    /// Construct a descriptor for a reply.
    #[must_use]
    pub const fn reply(protocol_name: &'static str, message_format: &'static str) -> Self {
        Self {
            kind: ItemKind::Reply,
            protocol_name,
            message_format,
        }
    }

    /// Construct a descriptor for an output.
    #[must_use]
    pub const fn output(protocol_name: &'static str, message_format: &'static str) -> Self {
        Self {
            kind: ItemKind::Output,
            protocol_name,
            message_format,
        }
    }

    /// Return the item kind.
    #[must_use]
    pub const fn kind(&self) -> ItemKind {
        self.kind
    }

    /// Return the protocol name that identifies this message on the wire.
    #[must_use]
    pub const fn protocol_name(&self) -> &'static str {
        self.protocol_name
    }

    /// Return the full Klipper-style message format string.
    #[must_use]
    pub const fn message_format(&self) -> &'static str {
        self.message_format
    }
}

/// ASCII-bytewise comparison of two `&str`s, usable in `const` contexts
/// (though `Ord` itself is not `const`).
fn cmp_str(a: &str, b: &str) -> Ordering {
    a.as_bytes().cmp(b.as_bytes())
}

impl Ord for MessageDescriptor {
    fn cmp(&self, other: &Self) -> Ordering {
        match self.kind.cmp(&other.kind) {
            Ordering::Equal => match cmp_str(self.protocol_name, other.protocol_name) {
                Ordering::Equal => cmp_str(self.message_format, other.message_format),
                o => o,
            },
            o => o,
        }
    }
}

impl PartialOrd for MessageDescriptor {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Descriptor for a typed reply payload.
///
/// Replies are sent in response to a command. Each reply type has a stable
/// wire format expressed as a Klipper message format string.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct ReplyDescriptor {
    protocol_name: &'static str,
    message_format: &'static str,
}

impl ReplyDescriptor {
    /// Construct a reply descriptor.
    #[must_use]
    pub const fn new(protocol_name: &'static str, message_format: &'static str) -> Self {
        Self {
            protocol_name,
            message_format,
        }
    }

    /// Return the protocol name that identifies this reply on the wire.
    #[must_use]
    pub const fn protocol_name(&self) -> &'static str {
        self.protocol_name
    }

    /// Return the full Klipper-style message format string.
    #[must_use]
    pub const fn message_format(&self) -> &'static str {
        self.message_format
    }
}

/// Descriptor for an unsolicited output payload.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct OutputDescriptor {
    protocol_name: &'static str,
    message_format: &'static str,
}

impl OutputDescriptor {
    /// Construct an output descriptor.
    #[must_use]
    pub const fn new(protocol_name: &'static str, message_format: &'static str) -> Self {
        Self {
            protocol_name,
            message_format,
        }
    }

    /// Return the protocol name that identifies this output on the wire.
    #[must_use]
    pub const fn protocol_name(&self) -> &'static str {
        self.protocol_name
    }

    /// Return the full Klipper-style message format string.
    #[must_use]
    pub const fn message_format(&self) -> &'static str {
        self.message_format
    }
}

/// Category of a [`DefinitionDescriptor`].
///
/// Enumerations map names to numeric values; constants map names to
/// scalar values. The assembler writes both kinds into the Klipper data
/// dictionary.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum DefinitionKind {
    /// A named enumeration exported to the data dictionary.
    Enumeration,
    /// A named constant exported to the data dictionary.
    Constant,
}

/// Descriptor for a constant or enumeration exported into the data
/// dictionary.
///
/// The `value` is carried as a `&'static str` so that providers can emit
/// arbitrary scalar or structured representations; the assembler parses
/// the string when composing the dictionary JSON.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct DefinitionDescriptor {
    kind: DefinitionKind,
    exported_name: &'static str,
    value: &'static str,
}

impl DefinitionDescriptor {
    /// Construct a definition descriptor.
    #[must_use]
    pub const fn new(
        kind: DefinitionKind,
        exported_name: &'static str,
        value: &'static str,
    ) -> Self {
        Self {
            kind,
            exported_name,
            value,
        }
    }

    /// Return the definition kind.
    #[must_use]
    pub const fn kind(&self) -> DefinitionKind {
        self.kind
    }

    /// Return the exported name.
    #[must_use]
    pub const fn exported_name(&self) -> &'static str {
        self.exported_name
    }

    /// Return the exported value as a string.
    #[must_use]
    pub const fn value(&self) -> &'static str {
        self.value
    }
}
