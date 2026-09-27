//! Descriptors that the item-level macros emit for each protocol item.
//!
//! Each `#[klipper_reply]`, `#[klipper_output]`, `#[klipper_constant]` and
//! `klipper_enumeration!` item gets a `const fn` returning its descriptor.
//! The assembler builds the dispatch table and data dictionary from the
//! items' carrier macros and generated constants, not from these values.

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
/// `value` is provider metadata; the assembler builds the dictionary from each
/// item's generated value constant, not from this field. For an enumeration it
/// is the dictionary JSON object; for a constant it is the raw value (a string
/// constant without JSON quoting).
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
